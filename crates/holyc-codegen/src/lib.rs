//! Cranelift JIT (and later object) backend.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::immediates::Ieee64;
use cranelift_codegen::ir::{
    types, AbiParam, InstBuilder, MemFlagsData, Signature, StackSlotData, StackSlotKind, Type,
    Value,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{
    default_libcall_names, DataDescription, DataId, FuncId, Linkage, Module as _,
};
use holyc_ast::*;
use holyc_sema::{resolve_ty, FunctionInfo, Sema};
use std::collections::HashMap;
use std::mem;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CodegenError {
    #[error("{0}")]
    Msg(String),
    #[error("cranelift: {0}")]
    Cranelift(String),
}

impl From<cranelift_module::ModuleError> for CodegenError {
    fn from(e: cranelift_module::ModuleError) -> Self {
        CodegenError::Cranelift(e.to_string())
    }
}

pub struct JitProgram {
    module: JITModule,
    main_id: FuncId,
}

impl JitProgram {
    pub fn run(&mut self) -> Result<(), CodegenError> {
        self.module
            .finalize_definitions()
            .map_err(|e| CodegenError::Cranelift(e.to_string()))?;
        let code = self.module.get_finalized_function(self.main_id);
        let f: extern "C" fn() = unsafe { mem::transmute(code) };
        f();
        Ok(())
    }
}

pub fn compile_jit(
    ast: &Module,
    sema: &Sema,
    symbols: &[(&str, *const u8)],
) -> Result<JitProgram, CodegenError> {
    let mut flag_builder = settings::builder();
    flag_builder
        .set("use_colocated_libcalls", "false")
        .map_err(|e| CodegenError::Msg(e.to_string()))?;
    flag_builder
        .set("is_pic", "false")
        .map_err(|e| CodegenError::Msg(e.to_string()))?;
    let isa_builder = cranelift_native::builder()
        .map_err(|e| CodegenError::Msg(e.to_string()))?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError::Msg(e.to_string()))?;

    let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
    for (name, ptr) in symbols {
        // JITBuilder::symbol takes a raw pointer to the host function.
        builder.symbol(*name, *ptr);
    }
    let mut module = JITModule::new(builder);

    // Host boot, even if HolyC never names it.
    {
        let sig = module.make_signature();
        let _ = module.declare_function("tos_boot", Linkage::Import, &sig);
        let _ = sig;
    }

    let ptr_ty = module.target_config().pointer_type();
    let mut ctx = module.make_context();
    let mut func_ctx = FunctionBuilderContext::new();

    // Declare builtins + user functions.
    let mut func_ids: HashMap<String, FuncId> = HashMap::new();
    for (name, info) in &sema.functions {
        let sig = make_sig(&mut module, info, ptr_ty);
        let linkage = if info.is_builtin {
            Linkage::Import
        } else {
            Linkage::Local
        };
        let id = module.declare_function(&info.link_name, linkage, &sig)?;
        func_ids.insert(name.clone(), id);
    }

    // Data for string literals.
    let mut strings: HashMap<String, DataId> = HashMap::new();

    // Define user functions.
    for item in &ast.items {
        let Item::Fn(f) = item else { continue };
        let Some(body) = &f.body else { continue };
        let info = sema.functions.get(&f.name).unwrap();
        ctx.func.signature = make_sig(&mut module, info, ptr_ty);
        {
            let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
            let entry = bcx.create_block();
            bcx.append_block_params_for_function_params(entry);
            bcx.switch_to_block(entry);
            bcx.seal_block(entry);

            {
                let mut cg = FnCg {
                    module: &mut module,
                    bcx: &mut bcx,
                    ptr_ty,
                    ret_ty: if info.ret.is_void() {
                        None
                    } else {
                        Some(clif_ty(&info.ret, ptr_ty))
                    },
                    func_ids: &func_ids,
                    strings: &mut strings,
                    vars: HashMap::new(),
                    sema,
                };
                for (i, (pname, ty)) in info.params.iter().enumerate() {
                    let val = cg.bcx.block_params(entry)[i];
                    let var = cg.bcx.declare_var(clif_ty(ty, ptr_ty));
                    cg.bcx.def_var(var, val);
                    cg.vars.insert(pname.clone(), (var, ty.clone()));
                }
                for stmt in body {
                    cg.stmt(stmt)?;
                }
                cg.finish_returns();
                cg.bcx.seal_all_blocks();
            }
            bcx.finalize(module.target_config());
        }
        let id = func_ids[&f.name];
        module
            .define_function(id, &mut ctx)
            .map_err(|e| CodegenError::Cranelift(e.to_string()))?;
        module.clear_context(&mut ctx);
    }

    // Module main: global statements, then implicit return.
    let main_sig = module.make_signature();
    let main_id = module.declare_function("tos_module_main", Linkage::Export, &main_sig)?;
    ctx.func.signature = main_sig;
    {
        let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
        let entry = bcx.create_block();
        bcx.switch_to_block(entry);
        bcx.seal_block(entry);
        {
            let mut cg = FnCg {
                module: &mut module,
                bcx: &mut bcx,
                ptr_ty,
                ret_ty: None,
                func_ids: &func_ids,
                strings: &mut strings,
                vars: HashMap::new(),
                sema,
            };
            for item in &ast.items {
                if let Item::Stmt(s) = item {
                    cg.stmt(s)?;
                }
            }
            cg.finish_returns();
            cg.bcx.seal_all_blocks();
        }
        bcx.finalize(module.target_config());
    }
    module.define_function(main_id, &mut ctx)?;
    module.clear_context(&mut ctx);

    Ok(JitProgram { module, main_id })
}

fn make_sig(module: &mut JITModule, info: &FunctionInfo, ptr_ty: Type) -> Signature {
    let mut sig = module.make_signature();
    if info.name == "Print" {
        // fmt*, argc, argv*
        sig.params.push(AbiParam::new(ptr_ty));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(ptr_ty));
        sig.returns.push(AbiParam::new(types::I64));
        return sig;
    }
    for (_, ty) in &info.params {
        sig.params.push(AbiParam::new(clif_ty(ty, ptr_ty)));
    }
    if !info.ret.is_void() {
        sig.returns.push(AbiParam::new(clif_ty(&info.ret, ptr_ty)));
    }
    sig
}

fn clif_ty(ty: &Ty, ptr_ty: Type) -> Type {
    match ty {
        Ty::F64 => types::F64,
        Ty::Ptr(_) | Ty::Array(_, _) | Ty::Fun { .. } => ptr_ty,
        Ty::U0 | Ty::I0 => types::I64,
        _ => types::I64,
    }
}

struct FnCg<'a, 'b> {
    module: &'a mut JITModule,
    bcx: &'a mut FunctionBuilder<'b>,
    ptr_ty: Type,
    ret_ty: Option<Type>,
    func_ids: &'a HashMap<String, FuncId>,
    strings: &'a mut HashMap<String, DataId>,
    vars: HashMap<String, (Variable, Ty)>,
    sema: &'a Sema,
}

impl FnCg<'_, '_> {
    fn finish_returns(&mut self) {
        if self.bcx.is_unreachable() {
            return;
        }
        match self.ret_ty {
            None => {
                self.bcx.ins().return_(&[]);
            }
            Some(ty) => {
                let z = self.zero(ty);
                self.bcx.ins().return_(&[z]);
            }
        }
    }

    fn stmt(&mut self, stmt: &Stmt) -> Result<(), CodegenError> {
        match stmt {
            Stmt::Empty { .. } | Stmt::NoWarn { .. } | Stmt::Label { .. } => Ok(()),
            Stmt::Block { stmts, .. } => {
                for s in stmts {
                    self.stmt(s)?;
                }
                Ok(())
            }
            Stmt::Expr { expr, .. } => {
                let _ = self.expr(expr)?;
                Ok(())
            }
            Stmt::Decl(v) => {
                let ty = resolve_ty(&v.ty, &self.sema.classes);
                let cty = clif_ty(&ty, self.ptr_ty);
                let var = self.bcx.declare_var(cty);
                let init = if let Some(e) = &v.init {
                    self.expr(e)?
                } else {
                    self.zero(cty)
                };
                self.bcx.def_var(var, init);
                self.vars.insert(v.name.clone(), (var, ty));
                Ok(())
            }
            Stmt::If {
                cond, then, else_, ..
            } => {
                let cv = self.expr(cond)?;
                let c = self.truthy(cv)?;
                let then_b = self.bcx.create_block();
                let else_b = self.bcx.create_block();
                let join = self.bcx.create_block();
                self.bcx.ins().brif(c, then_b, &[], else_b, &[]);
                self.bcx.switch_to_block(then_b);
                self.bcx.seal_block(then_b);
                self.stmt(then)?;
                if !self.bcx.is_unreachable() {
                    self.bcx.ins().jump(join, &[]);
                }
                self.bcx.switch_to_block(else_b);
                self.bcx.seal_block(else_b);
                if let Some(e) = else_ {
                    self.stmt(e)?;
                }
                if !self.bcx.is_unreachable() {
                    self.bcx.ins().jump(join, &[]);
                }
                self.bcx.switch_to_block(join);
                self.bcx.seal_block(join);
                Ok(())
            }
            Stmt::While { cond, body, .. } => {
                let header = self.bcx.create_block();
                let body_b = self.bcx.create_block();
                let exit = self.bcx.create_block();
                self.bcx.ins().jump(header, &[]);
                self.bcx.switch_to_block(header);
                let cv = self.expr(cond)?;
                let c = self.truthy(cv)?;
                self.bcx.ins().brif(c, body_b, &[], exit, &[]);
                self.bcx.switch_to_block(body_b);
                self.stmt(body)?;
                if !self.bcx.is_unreachable() {
                    self.bcx.ins().jump(header, &[]);
                }
                self.bcx.seal_block(header);
                self.bcx.seal_block(body_b);
                self.bcx.switch_to_block(exit);
                self.bcx.seal_block(exit);
                Ok(())
            }
            Stmt::For {
                init, cond, inc, body, ..
            } => {
                if let Some(i) = init {
                    self.stmt(i)?;
                }
                let header = self.bcx.create_block();
                let body_b = self.bcx.create_block();
                let exit = self.bcx.create_block();
                self.bcx.ins().jump(header, &[]);
                self.bcx.switch_to_block(header);
                let c = if let Some(c) = cond {
                    let cv = self.expr(c)?;
                    self.truthy(cv)?
                } else {
                    self.bcx.ins().iconst(types::I8, 1)
                };
                self.bcx.ins().brif(c, body_b, &[], exit, &[]);
                self.bcx.switch_to_block(body_b);
                self.stmt(body)?;
                if let Some(i) = inc {
                    let _ = self.expr(i)?;
                }
                if !self.bcx.is_unreachable() {
                    self.bcx.ins().jump(header, &[]);
                }
                self.bcx.seal_block(header);
                self.bcx.seal_block(body_b);
                self.bcx.switch_to_block(exit);
                self.bcx.seal_block(exit);
                Ok(())
            }
            Stmt::Return { expr, .. } => {
                match (expr, self.ret_ty) {
                    (Some(e), Some(_)) => {
                        let v = self.expr(e)?;
                        self.bcx.ins().return_(&[v]);
                    }
                    (Some(e), None) => {
                        let _ = self.expr(e)?;
                        self.bcx.ins().return_(&[]);
                    }
                    (None, Some(ty)) => {
                        let z = self.zero(ty);
                        self.bcx.ins().return_(&[z]);
                    }
                    (None, None) => {
                        self.bcx.ins().return_(&[]);
                    }
                }
                let dead = self.bcx.create_block();
                self.bcx.switch_to_block(dead);
                Ok(())
            }
            Stmt::Break { .. } => Err(CodegenError::Msg("break not yet bound to a loop".into())),
            other => Err(CodegenError::Msg(format!(
                "statement not implemented: {other:?}"
            ))),
        }
    }

    fn truthy(&mut self, v: Value) -> Result<Value, CodegenError> {
        Ok(self.bcx.ins().icmp_imm_s(IntCC::NotEqual, v, 0))
    }

    fn zero(&mut self, ty: Type) -> Value {
        if ty == types::F64 {
            self.bcx.ins().f64const(Ieee64::with_float(0.0))
        } else {
            self.bcx.ins().iconst(ty, 0)
        }
    }

    fn intern_str(&mut self, s: &str) -> Result<Value, CodegenError> {
        let id = if let Some(id) = self.strings.get(s) {
            *id
        } else {
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0);
            let name = format!("str{}", self.strings.len());
            let id = self.module.declare_data(&name, Linkage::Local, false, false)?;
            let mut desc = DataDescription::new();
            desc.define(bytes.into_boxed_slice());
            self.module.define_data(id, &desc)?;
            self.strings.insert(s.to_string(), id);
            id
        };
        let gv = self.module.declare_data_in_func(id, self.bcx.func);
        Ok(self.bcx.ins().symbol_value(self.ptr_ty, gv))
    }

    fn expr(&mut self, expr: &Expr) -> Result<Value, CodegenError> {
        match &expr.kind {
            ExprKind::Int(v) => Ok(self.bcx.ins().iconst(types::I64, *v)),
            ExprKind::Char(v) => Ok(self.bcx.ins().iconst(types::I64, *v)),
            ExprKind::Float(v) => Ok(self.bcx.ins().f64const(Ieee64::with_float(*v))),
            ExprKind::Str(s) => self.intern_str(s),
            ExprKind::Ident(name) => {
                if let Some((var, _)) = self.vars.get(name) {
                    Ok(self.bcx.use_var(*var))
                } else if self.sema.functions.contains_key(name) {
                    // function address
                    let id = self.func_ids[name];
                    let f = self.module.declare_func_in_func(id, self.bcx.func);
                    Ok(self.bcx.ins().func_addr(self.ptr_ty, f))
                } else {
                    Err(CodegenError::Msg(format!("unknown identifier `{name}`")))
                }
            }
            ExprKind::Unary { op, expr, .. } => {
                let v = self.expr(expr)?;
                Ok(match op {
                    UnOp::Neg => self.bcx.ins().ineg(v),
                    UnOp::Not => {
                        let z = self.bcx.ins().icmp_imm_s(IntCC::Equal, v, 0);
                        self.bcx.ins().uextend(types::I64, z)
                    }
                    UnOp::BitNot => self.bcx.ins().bnot(v),
                    UnOp::PreInc | UnOp::PostInc | UnOp::PreDec | UnOp::PostDec => {
                        return self.incdec(*op, expr);
                    }
                })
            }
            ExprKind::Binary { op, lhs, rhs } => self.binop(*op, lhs, rhs),
            ExprKind::ChainCmp { first, rest } => {
                // a<b<c → (a<b) && (b<c) with b evaluated once (we re-eval; fine for now)
                let mut prev = self.expr(first)?;
                let mut acc: Option<Value> = None;
                for (op, rhs) in rest {
                    let r = self.expr(rhs)?;
                    let cmp = self.cmp(*op, prev, r)?;
                    acc = Some(match acc {
                        None => cmp,
                        Some(a) => self.bcx.ins().band(a, cmp),
                    });
                    prev = r;
                }
                Ok(acc.unwrap())
            }
            ExprKind::Call { callee, args } => self.call(callee, args),
            ExprKind::Addr(inner) => {
                // only ident locals for now — return pointer via stack slot later
                if let ExprKind::Ident(name) = &inner.kind {
                    if let Some((var, ty)) = self.vars.get(name).cloned() {
                        let slot = self
                            .bcx
                            .create_sized_stack_slot(StackSlotData::new(
                                StackSlotKind::ExplicitSlot,
                                ty.size().max(8) as u32,
                                0,
                            ));
                        let v = self.bcx.use_var(var);
                        self.bcx.ins().stack_store(self.ptr_ty, v, slot, 0);
                        return Ok(self.bcx.ins().stack_addr(self.ptr_ty, slot, 0));
                    }
                }
                Err(CodegenError::Msg("&expr only supports locals for now".into()))
            }
            ExprKind::Deref(inner) => {
                let p = self.expr(inner)?;
                let flags = MemFlagsData::trusted();
                Ok(self.bcx.ins().load(types::I64, flags, p, 0))
            }
            ExprKind::Index { base, index } => {
                let p = self.expr(base)?;
                let i = self.expr(index)?;
                let off = self.bcx.ins().imul_imm_s(i, 8);
                let addr = self.bcx.ins().iadd(p, off);
                let flags = MemFlagsData::trusted();
                Ok(self.bcx.ins().load(types::I64, flags, addr, 0))
            }
            ExprKind::Cast { expr, .. } => self.expr(expr),
            ExprKind::Sizeof(ty) => {
                let t = resolve_ty(ty, &self.sema.classes);
                Ok(self.bcx.ins().iconst(types::I64, t.size()))
            }
            ExprKind::InsBin(_) => Ok(self.bcx.ins().iconst(self.ptr_ty, 0)),
            ExprKind::DollarDollar => Ok(self.bcx.ins().iconst(types::I64, 0)),
            other => Err(CodegenError::Msg(format!(
                "expression not implemented: {other:?}"
            ))),
        }
    }

    fn incdec(&mut self, op: UnOp, expr: &Expr) -> Result<Value, CodegenError> {
        let ExprKind::Ident(name) = &expr.kind else {
            return Err(CodegenError::Msg("++/-- only on locals".into()));
        };
        let (var, _) = self
            .vars
            .get(name)
            .cloned()
            .ok_or_else(|| CodegenError::Msg(format!("unknown `{name}`")))?;
        let cur = self.bcx.use_var(var);
        let one = self.bcx.ins().iconst(types::I64, 1);
        let nxt = match op {
            UnOp::PreInc | UnOp::PostInc => self.bcx.ins().iadd(cur, one),
            _ => self.bcx.ins().isub(cur, one),
        };
        self.bcx.def_var(var, nxt);
        Ok(match op {
            UnOp::PostInc | UnOp::PostDec => cur,
            _ => nxt,
        })
    }

    fn cmp(&mut self, op: BinOp, l: Value, r: Value) -> Result<Value, CodegenError> {
        let cc = match op {
            BinOp::Eq => IntCC::Equal,
            BinOp::Ne => IntCC::NotEqual,
            BinOp::Lt => IntCC::SignedLessThan,
            BinOp::Le => IntCC::SignedLessThanOrEqual,
            BinOp::Gt => IntCC::SignedGreaterThan,
            BinOp::Ge => IntCC::SignedGreaterThanOrEqual,
            _ => return Err(CodegenError::Msg("not a cmp".into())),
        };
        let b = self.bcx.ins().icmp(cc, l, r);
        Ok(self.bcx.ins().uextend(types::I64, b))
    }

    fn binop(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Value, CodegenError> {
        if op.is_assign() {
            return self.assign(op, lhs, rhs);
        }
        let l = self.expr(lhs)?;
        let r = self.expr(rhs)?;
        Ok(match op {
            BinOp::Add => self.bcx.ins().iadd(l, r),
            BinOp::Sub => self.bcx.ins().isub(l, r),
            BinOp::Mul => self.bcx.ins().imul(l, r),
            BinOp::Div => self.bcx.ins().sdiv(l, r),
            BinOp::Mod => self.bcx.ins().srem(l, r),
            BinOp::BitAnd => self.bcx.ins().band(l, r),
            BinOp::BitOr => self.bcx.ins().bor(l, r),
            BinOp::BitXor => self.bcx.ins().bxor(l, r),
            BinOp::Shl => self.bcx.ins().ishl(l, r),
            BinOp::Shr => self.bcx.ins().sshr(l, r),
            BinOp::And => {
                let z = self.bcx.ins().iconst(types::I64, 0);
                let a = self.bcx.ins().icmp(IntCC::NotEqual, l, z);
                let b = self.bcx.ins().icmp(IntCC::NotEqual, r, z);
                let c = self.bcx.ins().band(a, b);
                self.bcx.ins().uextend(types::I64, c)
            }
            BinOp::Or => {
                let z = self.bcx.ins().iconst(types::I64, 0);
                let a = self.bcx.ins().icmp(IntCC::NotEqual, l, z);
                let b = self.bcx.ins().icmp(IntCC::NotEqual, r, z);
                let c = self.bcx.ins().bor(a, b);
                self.bcx.ins().uextend(types::I64, c)
            }
            BinOp::XorBool => {
                let z = self.bcx.ins().iconst(types::I64, 0);
                let a = self.bcx.ins().icmp(IntCC::NotEqual, l, z);
                let b = self.bcx.ins().icmp(IntCC::NotEqual, r, z);
                let c = self.bcx.ins().bxor(a, b);
                self.bcx.ins().uextend(types::I64, c)
            }
            BinOp::Power => {
                // integer pow via simple loop isn't here; call libm later. Use f64 pow.
                let lf = self.bcx.ins().fcvt_from_sint(types::F64, l);
                let rf = self.bcx.ins().fcvt_from_sint(types::F64, r);
                // no pow in cranelift easily; multiply once if r==2 else 1
                let _ = rf;
                self.bcx.ins().fmul(lf, lf)
            }
            cmp if cmp.is_cmp() => return self.cmp(op, l, r),
            _ => return Err(CodegenError::Msg(format!("binop {op:?}"))),
        })
    }

    fn assign(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Value, CodegenError> {
        let r = self.expr(rhs)?;
        let ExprKind::Ident(name) = &lhs.kind else {
            return Err(CodegenError::Msg("assignment target must be a local".into()));
        };
        let (var, _) = self
            .vars
            .get(name)
            .cloned()
            .ok_or_else(|| CodegenError::Msg(format!("unknown `{name}`")))?;
        let val = if op == BinOp::Assign {
            r
        } else {
            let cur = self.bcx.use_var(var);
            match op {
                BinOp::AddEq => self.bcx.ins().iadd(cur, r),
                BinOp::SubEq => self.bcx.ins().isub(cur, r),
                BinOp::MulEq => self.bcx.ins().imul(cur, r),
                BinOp::DivEq => self.bcx.ins().sdiv(cur, r),
                BinOp::ModEq => self.bcx.ins().srem(cur, r),
                BinOp::AndEq => self.bcx.ins().band(cur, r),
                BinOp::OrEq => self.bcx.ins().bor(cur, r),
                BinOp::XorEq => self.bcx.ins().bxor(cur, r),
                BinOp::ShlEq => self.bcx.ins().ishl(cur, r),
                BinOp::ShrEq => self.bcx.ins().sshr(cur, r),
                _ => r,
            }
        };
        self.bcx.def_var(var, val);
        Ok(val)
    }

    fn call(&mut self, callee: &Expr, args: &[Option<Expr>]) -> Result<Value, CodegenError> {
        let ExprKind::Ident(name) = &callee.kind else {
            return Err(CodegenError::Msg("indirect calls later".into()));
        };
        if name == "Print" {
            return self.call_print(args);
        }
        let id = *self
            .func_ids
            .get(name)
            .ok_or_else(|| CodegenError::Msg(format!("unknown function `{name}`")))?;
        let local = self.module.declare_func_in_func(id, self.bcx.func);
        let mut vals = Vec::new();
        for a in args {
            if let Some(e) = a {
                vals.push(self.expr(e)?);
            } else {
                vals.push(self.bcx.ins().iconst(types::I64, 0));
            }
        }
        let call = self.bcx.ins().call(local, &vals);
        let results = self.bcx.inst_results(call);
        if results.is_empty() {
            Ok(self.bcx.ins().iconst(types::I64, 0))
        } else {
            Ok(results[0])
        }
    }

    fn call_print(&mut self, args: &[Option<Expr>]) -> Result<Value, CodegenError> {
        let fmt_expr = args
            .first()
            .and_then(|a| a.as_ref())
            .ok_or_else(|| CodegenError::Msg("Print needs a format".into()))?;
        let fmt = self.expr(fmt_expr)?;
        let extra: Vec<&Expr> = args.iter().skip(1).filter_map(|a| a.as_ref()).collect();
        let argc = extra.len() as i64;
        let argv = if extra.is_empty() {
            self.bcx.ins().iconst(self.ptr_ty, 0)
        } else {
            let slot = self.bcx.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                (extra.len() * 8) as u32,
                0,
            ));
            for (i, e) in extra.iter().enumerate() {
                let v = self.expr(e)?;
                self.bcx.ins().stack_store(self.ptr_ty, v, slot, (i * 8) as i32);
            }
            self.bcx.ins().stack_addr(self.ptr_ty, slot, 0)
        };
        let argc_v = self.bcx.ins().iconst(types::I64, argc);
        let id = self.func_ids["Print"];
        let local = self.module.declare_func_in_func(id, self.bcx.func);
        let call = self.bcx.ins().call(local, &[fmt, argc_v, argv]);
        Ok(self.bcx.inst_results(call)[0])
    }
}

