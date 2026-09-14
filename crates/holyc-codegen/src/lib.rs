//! Cranelift JIT (and later object) backend.

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::immediates::Ieee64;
use cranelift_codegen::ir::{
    AbiParam, Block, InstBuilder, MemFlagsData, Signature, StackSlot, StackSlotData, StackSlotKind,
    Type, Value, types,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{
    DataDescription, DataId, FuncId, Linkage, Module as _, default_libcall_names,
};
use holyc_ast::*;
use holyc_sema::{ClassInfo, FunctionInfo, Sema, resolve_ty};
use std::collections::{HashMap, HashSet};
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
    function_ids: HashMap<String, FuncId>,
    global_id: DataId,
    globals: HashMap<String, GlobalInfo>,
    finalized: bool,
}

/// Address and storage size of one finalized JIT global.
pub struct JitGlobal {
    pub name: String,
    pub address: *mut u8,
    pub size: usize,
}

impl JitProgram {
    fn finalize(&mut self) -> Result<(), CodegenError> {
        if self.finalized {
            return Ok(());
        }
        self.module
            .finalize_definitions()
            .map_err(|e| CodegenError::Cranelift(e.to_string()))?;
        self.finalized = true;
        Ok(())
    }

    /// Finalize the module and expose its packed globals to a compatibility
    /// host before the generated entry point starts executing. Returned
    /// addresses remain valid only while this `JitProgram` is alive.
    pub fn global_bindings(&mut self) -> Result<Vec<JitGlobal>, CodegenError> {
        self.finalize()?;
        let (base, allocation_size) = self.module.get_finalized_data(self.global_id);
        let mut bindings = Vec::with_capacity(self.globals.len());
        for (name, global) in &self.globals {
            let offset = usize::try_from(global.offset)
                .map_err(|_| CodegenError::Msg("negative global offset".into()))?;
            let size = usize::try_from(global.ty.size().max(1))
                .map_err(|_| CodegenError::Msg(format!("global `{name}` is too large")))?;
            if offset.saturating_add(size) > allocation_size {
                return Err(CodegenError::Msg(format!(
                    "global `{name}` exceeds its JIT allocation"
                )));
            }
            bindings.push(JitGlobal {
                name: name.clone(),
                address: unsafe { base.add(offset) }.cast_mut(),
                size,
            });
        }
        Ok(bindings)
    }

    pub fn run(&mut self) -> Result<(), CodegenError> {
        self.finalize()?;
        if std::env::var_os("SANCTUM_JIT_MAP").is_some() {
            let mut functions = self
                .function_ids
                .iter()
                .map(|(name, id)| (self.module.get_finalized_function(*id) as usize, name))
                .collect::<Vec<_>>();
            functions.sort_unstable_by_key(|(address, _)| *address);
            for (address, name) in functions {
                eprintln!("JIT {address:#018x} {name}");
            }
        }
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
    binary_resources: &HashMap<(u32, i64), Vec<u8>>,
) -> Result<JitProgram, CodegenError> {
    fn collect_global_order(
        stmt: &Stmt,
        globals: &HashMap<String, Ty>,
        ordered: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) {
        match stmt {
            Stmt::Decl(variable) => {
                if globals.contains_key(&variable.name) && ordered.insert(variable.name.clone()) {
                    order.push(variable.name.clone());
                }
            }
            Stmt::Block { stmts, .. } => {
                for stmt in stmts {
                    collect_global_order(stmt, globals, ordered, order);
                }
            }
            _ => {}
        }
    }

    let mut flag_builder = settings::builder();
    flag_builder
        .set("use_colocated_libcalls", "false")
        .map_err(|e| CodegenError::Msg(e.to_string()))?;
    flag_builder
        .set("is_pic", "false")
        .map_err(|e| CodegenError::Msg(e.to_string()))?;
    let isa_builder = cranelift_native::builder().map_err(|e| CodegenError::Msg(e.to_string()))?;
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
    declare_runtime_helpers(&mut module, &mut func_ids)?;

    // Data for string literals.
    let mut strings: HashMap<String, DataId> = HashMap::new();
    let mut global_order = Vec::new();
    let mut ordered = HashSet::new();
    for item in &ast.items {
        if let Item::Stmt(stmt) = item {
            collect_global_order(stmt, &sema.globals, &mut ordered, &mut global_order);
        }
    }
    let mut remaining = sema
        .globals
        .keys()
        .filter(|name| !ordered.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    remaining.sort();
    global_order.extend(remaining);

    // TempleOS module globals share one packed allocation. Besides matching
    // its address/adjacency semantics, this matters to old unchecked HolyC:
    // programs sometimes read just beyond one global array into its neighbor.
    let mut layout = Vec::new();
    let mut global_bytes = 0usize;
    for name in global_order {
        let ty = sema.globals[&name].clone();
        global_bytes = global_bytes
            .checked_add(7)
            .ok_or_else(|| CodegenError::Msg("global data size overflow".into()))?
            & !7;
        let size = usize::try_from(ty.size().max(1))
            .map_err(|_| CodegenError::Msg(format!("global `{name}` is too large")))?;
        layout.push((name, global_bytes, ty));
        global_bytes = global_bytes
            .checked_add(size)
            .ok_or_else(|| CodegenError::Msg("global data size overflow".into()))?;
    }
    if global_bytes > i32::MAX as usize {
        return Err(CodegenError::Msg("global data exceeds 2 GiB".into()));
    }
    let global_id = module.declare_data("module_globals", Linkage::Local, true, false)?;
    let global_image = vec![0u8; global_bytes.max(1)];
    let mut globals = HashMap::new();
    for (name, offset, ty) in layout {
        let offset = i32::try_from(offset)
            .map_err(|_| CodegenError::Msg("global data exceeds 2 GiB".into()))?;
        globals.insert(
            name,
            GlobalInfo {
                id: global_id,
                offset,
                ty,
            },
        );
    }
    let mut global_desc = DataDescription::new();
    global_desc.define(global_image.into_boxed_slice());
    module.define_data(global_id, &global_desc)?;
    let mut bins: HashMap<(u32, i64), DataId> = HashMap::new();
    for ((file, idx), bytes) in binary_resources {
        let id = module.declare_data(&format!("bin_{file}_{idx}"), Linkage::Local, false, false)?;
        let mut desc = DataDescription::new();
        desc.define(bytes.clone().into_boxed_slice());
        module.define_data(id, &desc)?;
        bins.insert((*file, *idx), id);
    }

    let mut fn_defs = Vec::new();
    for item in &ast.items {
        if let Item::Fn(function) = item {
            collect_fn_defs(function, None, &mut fn_defs);
        }
    }
    let mut env_layouts = HashMap::new();
    for (link, function) in &fn_defs {
        if let Some(layout) = env_layout_for(link, function, sema) {
            env_layouts.insert(link.clone(), layout);
        }
    }

    // Define user functions, including nested ones.
    for (link, f) in &fn_defs {
        let Some(body) = &f.body else { continue };
        let info = sema
            .functions
            .get(link)
            .or_else(|| sema.functions.get(&f.name))
            .unwrap();
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
                    bins: &bins,
                    vars: HashMap::new(),
                    globals: &globals,
                    module_scope: false,
                    break_targets: Vec::new(),
                    catch_targets: Vec::new(),
                    labels: HashMap::new(),
                    current_link: link.clone(),
                    env_slot: None,
                    env_offsets: HashMap::new(),
                    env_pushed: false,
                    env_layouts: &env_layouts,
                    sema,
                };
                cg.begin_env()?;
                for (i, (pname, ty)) in info.params.iter().enumerate() {
                    let val = cg.bcx.block_params(entry)[i];
                    cg.define_local(pname.clone(), ty.clone(), val)?;
                }
                cg.import_captures()?;
                cg.prepare_labels(body)?;
                for stmt in body {
                    cg.stmt(stmt)?;
                }
                cg.finish_returns();
                cg.bcx.seal_all_blocks();
            }
            bcx.finalize(module.target_config());
        }
        let id = func_ids
            .get(link)
            .or_else(|| func_ids.get(&f.name))
            .copied()
            .ok_or_else(|| CodegenError::Msg(format!("missing function id for `{link}`")))?;
        module
            .define_function(id, &mut ctx)
            .map_err(|e| CodegenError::Cranelift(format!("{e:?}")))?;
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
                bins: &bins,
                vars: HashMap::new(),
                globals: &globals,
                module_scope: true,
                break_targets: Vec::new(),
                catch_targets: Vec::new(),
                labels: HashMap::new(),
                current_link: String::new(),
                env_slot: None,
                env_offsets: HashMap::new(),
                env_pushed: false,
                env_layouts: &env_layouts,
                sema,
            };
            cg.prepare_module_labels(&ast.items)?;
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
    module
        .define_function(main_id, &mut ctx)
        .map_err(|e| CodegenError::Cranelift(format!("{e:?}")))?;
    module.clear_context(&mut ctx);

    let function_ids = ast
        .items
        .iter()
        .filter_map(|item| {
            let Item::Fn(function) = item else {
                return None;
            };
            function
                .body
                .as_ref()
                .map(|_| (function.name.clone(), func_ids[&function.name]))
        })
        .collect();
    Ok(JitProgram {
        module,
        main_id,
        function_ids,
        global_id,
        globals,
        finalized: false,
    })
}

fn declare_runtime_helpers(
    module: &mut JITModule,
    func_ids: &mut HashMap<String, FuncId>,
) -> Result<(), CodegenError> {
    let ptr_ty = module.target_config().pointer_type();
    let mut declare =
        |name: &str, params: &[Type], ret: Option<Type>| -> Result<(), CodegenError> {
            let mut sig = module.make_signature();
            for ty in params {
                sig.params.push(AbiParam::new(*ty));
            }
            if let Some(ty) = ret {
                sig.returns.push(AbiParam::new(ty));
            }
            let id = module.declare_function(name, Linkage::Import, &sig)?;
            func_ids.insert(name.to_string(), id);
            Ok(())
        };
    declare("tos_HasExcept", &[], Some(types::I64))?;
    declare("tos_Throw", &[types::I64], None)?;
    declare("tos_ClearExcept", &[], None)?;
    declare("tos_PowI64", &[types::I64, types::I64], Some(types::I64))?;
    declare("tos_PowF64", &[types::F64, types::F64], Some(types::F64))?;
    declare("tos_EnvPush", &[ptr_ty], None)?;
    declare("tos_EnvPop", &[], None)?;
    declare("tos_EnvPeek", &[], Some(ptr_ty))?;
    Ok(())
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
    if info.name == "GrPrint" {
        // dc*, x, y, fmt*, argc, argv*
        sig.params.push(AbiParam::new(ptr_ty));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
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
        Ty::Ptr(_) | Ty::Array(_, _) | Ty::Fun { .. } | Ty::Class { .. } => ptr_ty,
        Ty::U0 | Ty::I0 => types::I64,
        _ => types::I64,
    }
}

fn mem_clif(ty: &Ty) -> Type {
    match ty {
        Ty::F64 => types::F64,
        Ty::I8 | Ty::U8 => types::I8,
        Ty::I16 | Ty::U16 => types::I16,
        Ty::I32 | Ty::U32 => types::I32,
        _ => types::I64,
    }
}

fn scalar_lane_view(ty: &Ty, name: &str) -> Option<Ty> {
    if !ty.is_int() {
        return None;
    }
    let elem = match name {
        "i8" => Ty::I8,
        "u8" => Ty::U8,
        "i16" => Ty::I16,
        "u16" => Ty::U16,
        "i32" => Ty::I32,
        "u32" => Ty::U32,
        "i64" => Ty::I64,
        "u64" => Ty::U64,
        _ => return None,
    };
    Some(Ty::Array(
        Box::new(elem.clone()),
        Some(ty.size().max(8) / elem.size()),
    ))
}

struct FnCg<'a, 'b> {
    module: &'a mut JITModule,
    bcx: &'a mut FunctionBuilder<'b>,
    ptr_ty: Type,
    ret_ty: Option<Type>,
    func_ids: &'a HashMap<String, FuncId>,
    strings: &'a mut HashMap<String, DataId>,
    bins: &'a HashMap<(u32, i64), DataId>,
    vars: HashMap<String, (LocalStorage, Ty)>,
    globals: &'a HashMap<String, GlobalInfo>,
    module_scope: bool,
    break_targets: Vec<Block>,
    catch_targets: Vec<Block>,
    labels: HashMap<String, Block>,
    current_link: String,
    env_slot: Option<StackSlot>,
    env_offsets: HashMap<String, i32>,
    env_pushed: bool,
    env_layouts: &'a HashMap<String, EnvLayout>,
    sema: &'a Sema,
}

#[derive(Clone)]
struct GlobalInfo {
    id: DataId,
    offset: i32,
    ty: Ty,
}

#[derive(Clone, Copy)]
enum LocalStorage {
    Value(Variable),
    Stack(StackSlot),
    Env(i32),
    Captured { depth: u32, offset: i32 },
}

struct EnvLayout {
    slots: HashMap<String, (i32, Ty)>,
    size: u32,
}

fn collect_fn_defs<'a>(
    function: &'a FnDecl,
    parent: Option<&str>,
    out: &mut Vec<(String, &'a FnDecl)>,
) {
    let link = match parent {
        Some(parent) => Sema::nested_link_name(parent, &function.name),
        None => function.name.clone(),
    };
    out.push((link.clone(), function));
    if let Some(body) = &function.body {
        for stmt in body {
            walk_nested_fns(stmt, &link, out);
        }
    }
}

fn walk_nested_fns<'a>(stmt: &'a Stmt, parent: &str, out: &mut Vec<(String, &'a FnDecl)>) {
    match stmt {
        Stmt::Fn(function) => collect_fn_defs(function, Some(parent), out),
        Stmt::Block { stmts, .. } | Stmt::Start { body: stmts, .. } => {
            for stmt in stmts {
                walk_nested_fns(stmt, parent, out);
            }
        }
        Stmt::If { then, else_, .. } => {
            walk_nested_fns(then, parent, out);
            if let Some(else_) = else_ {
                walk_nested_fns(else_, parent, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Switch { body, .. } => walk_nested_fns(body, parent, out),
        Stmt::Try { body, catch, .. } => {
            walk_nested_fns(body, parent, out);
            walk_nested_fns(catch, parent, out);
        }
        _ => {}
    }
}

fn stmt_has_nested_fn(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Fn(_) => true,
        Stmt::Block { stmts, .. } | Stmt::Start { body: stmts, .. } => {
            stmts.iter().any(stmt_has_nested_fn)
        }
        Stmt::If { then, else_, .. } => {
            stmt_has_nested_fn(then) || else_.as_deref().is_some_and(stmt_has_nested_fn)
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Switch { body, .. } => stmt_has_nested_fn(body),
        Stmt::Try { body, catch, .. } => stmt_has_nested_fn(body) || stmt_has_nested_fn(catch),
        _ => false,
    }
}

fn collect_env_decls(
    stmt: &Stmt,
    classes: &HashMap<String, ClassInfo>,
    slots: &mut HashMap<String, (i32, Ty)>,
    offset: &mut i32,
) {
    match stmt {
        Stmt::Decl(variable) => {
            if slots.contains_key(&variable.name) {
                return;
            }
            let ty = resolve_ty(&variable.ty, classes);
            let size = ty.size().max(8);
            *offset = (*offset + 7) & !7;
            slots.insert(variable.name.clone(), (*offset, ty));
            *offset += size as i32;
        }
        Stmt::Fn(_) => {}
        Stmt::Block { stmts, .. } | Stmt::Start { body: stmts, .. } => {
            for stmt in stmts {
                collect_env_decls(stmt, classes, slots, offset);
            }
        }
        Stmt::If { then, else_, .. } => {
            collect_env_decls(then, classes, slots, offset);
            if let Some(else_) = else_ {
                collect_env_decls(else_, classes, slots, offset);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Switch { body, .. } => {
            collect_env_decls(body, classes, slots, offset);
        }
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                collect_env_decls(init, classes, slots, offset);
            }
            collect_env_decls(body, classes, slots, offset);
        }
        Stmt::Try { body, catch, .. } => {
            collect_env_decls(body, classes, slots, offset);
            collect_env_decls(catch, classes, slots, offset);
        }
        _ => {}
    }
}

fn env_layout_for(link: &str, function: &FnDecl, sema: &Sema) -> Option<EnvLayout> {
    let body = function.body.as_ref()?;
    if !body.iter().any(stmt_has_nested_fn) {
        return None;
    }
    let mut offset = 8_i32;
    let mut slots = HashMap::new();
    if let Some(info) = sema.functions.get(link) {
        for (name, ty) in &info.params {
            let size = ty.size().max(8);
            offset = (offset + 7) & !7;
            slots.insert(name.clone(), (offset, ty.clone()));
            offset += size as i32;
        }
    }
    for stmt in body {
        collect_env_decls(stmt, &sema.classes, &mut slots, &mut offset);
    }
    offset = (offset + 7) & !7;
    Some(EnvLayout {
        slots,
        size: offset.max(8) as u32,
    })
}

#[derive(Clone, Copy)]
struct FlatSwitchStmt<'a> {
    stmt: &'a Stmt,
    group: Option<usize>,
}

#[derive(Clone, Copy, Default)]
struct FlatSwitchGroup {
    start: usize,
    end: usize,
}

fn flatten_switch_body<'a>(
    stmt: &'a Stmt,
    out: &mut Vec<FlatSwitchStmt<'a>>,
    groups: &mut Vec<FlatSwitchGroup>,
    group: Option<usize>,
) {
    match stmt {
        Stmt::Block { stmts, .. } => {
            for stmt in stmts {
                flatten_switch_body(stmt, out, groups, group);
            }
        }
        Stmt::Start { body, .. } => {
            let id = groups.len();
            groups.push(FlatSwitchGroup::default());
            let start = out.len();
            for stmt in body {
                flatten_switch_body(stmt, out, groups, Some(id));
            }
            groups[id] = FlatSwitchGroup {
                start,
                end: out.len(),
            };
        }
        stmt => out.push(FlatSwitchStmt { stmt, group }),
    }
}

fn gather_labels(stmt: &Stmt, names: &mut Vec<String>) {
    match stmt {
        Stmt::Label { name, .. } => names.push(name.clone()),
        Stmt::Block { stmts, .. } | Stmt::Start { body: stmts, .. } => {
            for stmt in stmts {
                gather_labels(stmt, names);
            }
        }
        Stmt::If { then, else_, .. } => {
            gather_labels(then, names);
            if let Some(else_) = else_ {
                gather_labels(else_, names);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Switch { body, .. } => gather_labels(body, names),
        Stmt::Try { body, catch, .. } => {
            gather_labels(body, names);
            gather_labels(catch, names);
        }
        Stmt::Fn(_) => {}
        _ => {}
    }
}

impl FnCg<'_, '_> {
    fn prepare_labels(&mut self, stmts: &[Stmt]) -> Result<(), CodegenError> {
        let mut names = Vec::new();
        for stmt in stmts {
            gather_labels(stmt, &mut names);
        }
        self.define_labels(names)
    }

    fn prepare_module_labels(&mut self, items: &[Item]) -> Result<(), CodegenError> {
        let mut names = Vec::new();
        for item in items {
            if let Item::Stmt(stmt) = item {
                gather_labels(stmt, &mut names);
            }
        }
        self.define_labels(names)
    }

    fn define_labels(&mut self, names: Vec<String>) -> Result<(), CodegenError> {
        let mut seen = HashSet::new();
        for name in names {
            if !seen.insert(name.clone()) {
                return Err(CodegenError::Msg(format!("duplicate label `{name}`")));
            }
            let block = self.bcx.create_block();
            self.labels.insert(name, block);
        }
        Ok(())
    }

    fn call_import(&mut self, name: &str, args: &[Value]) -> Result<Vec<Value>, CodegenError> {
        let id = *self
            .func_ids
            .get(name)
            .ok_or_else(|| CodegenError::Msg(format!("missing runtime helper `{name}`")))?;
        let local = self.module.declare_func_in_func(id, self.bcx.func);
        let call = self.bcx.ins().call(local, args);
        Ok(self.bcx.inst_results(call).to_vec())
    }

    fn emit_unwind_return(&mut self) {
        let _ = self.pop_env();
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

    fn after_call(&mut self) -> Result<(), CodegenError> {
        if self.bcx.is_unreachable() {
            return Ok(());
        }
        let has = self.call_import("tos_HasExcept", &[])?;
        let cond = self.truthy(has[0])?;
        let cont = self.bcx.create_block();
        if let Some(catch) = self.catch_targets.last().copied() {
            self.bcx.ins().brif(cond, catch, &[], cont, &[]);
        } else {
            let unwind = self.bcx.create_block();
            self.bcx.ins().brif(cond, unwind, &[], cont, &[]);
            self.bcx.switch_to_block(unwind);
            self.bcx.seal_block(unwind);
            self.emit_unwind_return();
        }
        self.bcx.switch_to_block(cont);
        self.bcx.seal_block(cont);
        Ok(())
    }

    fn missing_arg(&mut self, name: &str, index: usize, ty: &Ty) -> Value {
        match (name, index) {
            ("Spawn", 3) => self.bcx.ins().iconst(types::I64, -1),
            ("Spawn", 6) => self.bcx.ins().iconst(types::I64, 1),
            ("Wrap", 1) => self
                .bcx
                .ins()
                .f64const(Ieee64::with_float(-std::f64::consts::PI)),
            ("Beep", 0) => self.bcx.ins().iconst(types::I64, 62),
            ("DCFill", 1) => self.bcx.ins().iconst(types::I64, 0xff),
            ("GrCircle3", 5) => self.bcx.ins().iconst(types::I64, 1),
            ("GrCircle3", 6) => self.bcx.ins().f64const(Ieee64::with_float(0.0)),
            ("GrCircle3", 7) => self
                .bcx
                .ins()
                .f64const(Ieee64::with_float(std::f64::consts::TAU)),
            _ => self.zero(clif_ty(ty, self.ptr_ty)),
        }
    }

    fn lookup_fn(&self, name: &str) -> Option<&FunctionInfo> {
        self.sema.lookup_function_from(name, &self.current_link)
    }

    fn func_id(&self, name: &str) -> Result<FuncId, CodegenError> {
        let info = self
            .lookup_fn(name)
            .ok_or_else(|| CodegenError::Msg(format!("unknown function `{name}`")))?;
        self.func_ids
            .get(&info.link_name)
            .or_else(|| self.func_ids.get(&info.name))
            .copied()
            .ok_or_else(|| CodegenError::Msg(format!("missing function id for `{name}`")))
    }

    fn begin_env(&mut self) -> Result<(), CodegenError> {
        let Some((size, offsets)) = self.env_layouts.get(&self.current_link).map(|layout| {
            (
                layout.size,
                layout
                    .slots
                    .iter()
                    .map(|(name, (offset, _))| (name.clone(), *offset))
                    .collect::<HashMap<_, _>>(),
            )
        }) else {
            return Ok(());
        };
        let slot = self.bcx.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            size,
            3,
        ));
        let ptr = self.bcx.ins().stack_addr(self.ptr_ty, slot, 0);
        self.memset(ptr, 0, i64::from(size))?;
        self.env_offsets = offsets;
        self.env_slot = Some(slot);
        self.env_pushed = true;
        let _ = self.call_import("tos_EnvPush", &[ptr])?;
        Ok(())
    }

    fn import_captures(&mut self) -> Result<(), CodegenError> {
        let mut depth = u32::from(self.env_pushed);
        let mut parent = self
            .sema
            .functions
            .get(&self.current_link)
            .and_then(|info| info.parent_link.clone());
        while let Some(link) = parent {
            let captured = self.env_layouts.get(&link).map(|layout| {
                layout
                    .slots
                    .iter()
                    .map(|(name, (offset, ty))| (name.clone(), *offset, ty.clone()))
                    .collect::<Vec<_>>()
            });
            if let Some(captured) = captured {
                for (name, offset, ty) in captured {
                    self.vars
                        .entry(name)
                        .or_insert((LocalStorage::Captured { depth, offset }, ty));
                }
            }
            parent = self
                .sema
                .functions
                .get(&link)
                .and_then(|info| info.parent_link.clone());
            depth += 1;
        }
        Ok(())
    }

    fn own_env_ptr(&mut self) -> Result<Value, CodegenError> {
        let slot = self
            .env_slot
            .ok_or_else(|| CodegenError::Msg("function environment is missing".into()))?;
        Ok(self.bcx.ins().stack_addr(self.ptr_ty, slot, 0))
    }

    fn env_at(&mut self, depth: u32) -> Result<Value, CodegenError> {
        let results = self.call_import("tos_EnvPeek", &[])?;
        let mut ptr = results[0];
        for _ in 0..depth {
            ptr = self
                .bcx
                .ins()
                .load(self.ptr_ty, MemFlagsData::trusted(), ptr, 0);
        }
        Ok(ptr)
    }

    fn pop_env(&mut self) -> Result<(), CodegenError> {
        if self.env_pushed && !self.bcx.is_unreachable() {
            let _ = self.call_import("tos_EnvPop", &[])?;
        }
        Ok(())
    }

    fn define_local(&mut self, name: String, ty: Ty, val: Value) -> Result<(), CodegenError> {
        if let Some(offset) = self.env_offsets.get(&name).copied() {
            let env = self.own_env_ptr()?;
            self.store_mem(env, offset, &ty, val)?;
            self.vars.insert(name, (LocalStorage::Env(offset), ty));
            return Ok(());
        }
        let storage = if ty.is_aggregate() {
            let var = self.bcx.declare_var(self.ptr_ty);
            self.bcx.def_var(var, val);
            LocalStorage::Value(var)
        } else {
            let slot = self.bcx.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                ty.size().max(8) as u32,
                3,
            ));
            let ptr = self.bcx.ins().stack_addr(self.ptr_ty, slot, 0);
            self.store_mem(ptr, 0, &ty, val)?;
            LocalStorage::Stack(slot)
        };
        self.vars.insert(name, (storage, ty));
        Ok(())
    }

    fn expr_ty(&self, e: &Expr) -> Option<Ty> {
        match &e.kind {
            ExprKind::Call { callee, .. } => match &callee.kind {
                ExprKind::Ident(n) => self.lookup_fn(n).map(|f| f.ret.clone()),
                _ => None,
            },
            ExprKind::Ident(n) => self
                .vars
                .get(n)
                .map(|(_, t)| t.clone())
                .or_else(|| self.globals.get(n).map(|global| global.ty.clone()))
                .or_else(|| self.lookup_fn(n).map(|f| f.ret.clone())),
            ExprKind::Field { base, name, .. } => {
                let bt = self.expr_ty(base)?;
                if let Some(view) = scalar_lane_view(&bt, name) {
                    return Some(view);
                }
                let cname = bt.class_name()?;
                self.sema
                    .classes
                    .get(cname)?
                    .member(name)
                    .map(|m| m.ty.clone())
            }
            ExprKind::Addr(inner) => Some(Ty::Ptr(Box::new(self.expr_ty(inner)?))),
            ExprKind::Deref(inner) => match self.expr_ty(inner)? {
                Ty::Ptr(t) => Some(*t),
                _ => None,
            },
            ExprKind::Index { base, .. } => match self.expr_ty(base)? {
                Ty::Array(elem, _) | Ty::Ptr(elem) => Some(*elem),
                _ => None,
            },
            ExprKind::Int(_) | ExprKind::Char(_) => Some(Ty::I64),
            ExprKind::Float(_) => Some(Ty::F64),
            ExprKind::Str(_) => Some(Ty::Ptr(Box::new(Ty::U8))),
            ExprKind::Unary { expr, .. } => self.expr_ty(expr),
            ExprKind::Binary { op, lhs, rhs } => {
                if op.is_assign() {
                    self.expr_ty(lhs)
                } else if op.is_cmp() || matches!(op, BinOp::And | BinOp::Or | BinOp::XorBool) {
                    Some(Ty::I64)
                } else if matches!(op, BinOp::Add | BinOp::Sub)
                    && matches!(self.expr_ty(lhs), Some(Ty::Ptr(_)))
                {
                    self.expr_ty(lhs)
                } else if op == &BinOp::Add && matches!(self.expr_ty(rhs), Some(Ty::Ptr(_))) {
                    self.expr_ty(rhs)
                } else if self.expr_ty(lhs)?.is_float() || self.expr_ty(rhs)?.is_float() {
                    Some(Ty::F64)
                } else {
                    Some(Ty::I64)
                }
            }
            ExprKind::ChainCmp { .. } => Some(Ty::I64),
            ExprKind::Sequence(exprs) => exprs.last().and_then(|expr| self.expr_ty(expr)),
            ExprKind::Cast { ty, .. } => Some(resolve_ty(ty, &self.sema.classes)),
            _ => None,
        }
    }

    fn value_ty(&self, value: Value) -> Type {
        self.bcx.func.dfg.value_type(value)
    }

    fn coerce_to(&mut self, value: Value, target: Type) -> Value {
        let source = self.value_ty(value);
        if source == target {
            value
        } else if target == types::F64 {
            self.bcx.ins().fcvt_from_sint(types::F64, value)
        } else if source == types::F64 {
            self.bcx.ins().fcvt_to_sint_sat(target, value)
        } else {
            value
        }
    }

    fn promote_numeric(&mut self, lhs: Value, rhs: Value) -> (Value, Value, bool) {
        let float = self.value_ty(lhs) == types::F64 || self.value_ty(rhs) == types::F64;
        if float {
            (
                self.coerce_to(lhs, types::F64),
                self.coerce_to(rhs, types::F64),
                true,
            )
        } else {
            (lhs, rhs, false)
        }
    }

    fn field_loc(&mut self, expr: &Expr) -> Result<(Value, Ty, i32), CodegenError> {
        let ExprKind::Field { base, name, .. } = &expr.kind else {
            return Err(CodegenError::Msg("not a field".into()));
        };
        let bt = self
            .expr_ty(base)
            .ok_or_else(|| CodegenError::Msg(format!("cannot type field base for `{name}`")))?;
        if let Some(view) = scalar_lane_view(&bt, name) {
            let (ptr, _, off) = self.place(base)?;
            return Ok((ptr, view, off));
        }
        let cname = bt
            .class_name()
            .ok_or_else(|| CodegenError::Msg(format!("`{name}` on non-class")))?
            .to_string();
        let member = self
            .sema
            .classes
            .get(&cname)
            .and_then(|c| c.member(name))
            .ok_or_else(|| CodegenError::Msg(format!("no member `{name}` on {cname}")))?;
        let ty = member.ty.clone();
        let off = member.offset as i32;
        let ptr = self.expr(base)?;
        Ok((ptr, ty, off))
    }

    fn place(&mut self, expr: &Expr) -> Result<(Value, Ty, i32), CodegenError> {
        match &expr.kind {
            ExprKind::Ident(name) => {
                if let Some((storage, ty)) = self.vars.get(name).cloned() {
                    let ptr = match storage {
                        LocalStorage::Value(var) if ty.is_aggregate() => self.bcx.use_var(var),
                        LocalStorage::Stack(slot) => {
                            self.bcx.ins().stack_addr(self.ptr_ty, slot, 0)
                        }
                        LocalStorage::Env(offset) => {
                            let env = self.own_env_ptr()?;
                            self.place_addr(env, offset)
                        }
                        LocalStorage::Captured { depth, offset } => {
                            let env = self.env_at(depth)?;
                            self.place_addr(env, offset)
                        }
                        LocalStorage::Value(_) => {
                            return Err(CodegenError::Msg(format!(
                                "local `{name}` is not addressable"
                            )));
                        }
                    };
                    return Ok((ptr, ty, 0));
                }
                if let Some(info) = self.globals.get(name).cloned() {
                    let global = self.module.declare_data_in_func(info.id, self.bcx.func);
                    let ptr = self.bcx.ins().symbol_value(self.ptr_ty, global);
                    return Ok((ptr, info.ty, info.offset));
                }
                Err(CodegenError::Msg(format!("unknown variable `{name}`")))
            }
            ExprKind::Field { .. } => self.field_loc(expr),
            ExprKind::Deref(inner) => {
                let ty = match self.expr_ty(inner) {
                    Some(Ty::Ptr(ty)) => *ty,
                    other => {
                        return Err(CodegenError::Msg(format!(
                            "cannot dereference non-pointer expression {:?} (type {other:?})",
                            inner.kind
                        )));
                    }
                };
                Ok((self.expr(inner)?, ty, 0))
            }
            ExprKind::Index { base, index } => {
                let elem = match self.expr_ty(base) {
                    Some(Ty::Array(elem, _)) | Some(Ty::Ptr(elem)) => *elem,
                    other => {
                        return Err(CodegenError::Msg(format!(
                            "cannot index expression {:?} with type {other:?}",
                            base.kind
                        )));
                    }
                };
                let base_ptr = self.expr(base)?;
                let index = self.expr(index)?;
                let byte_offset = self.bcx.ins().imul_imm_s(index, elem.size());
                let ptr = self.bcx.ins().iadd(base_ptr, byte_offset);
                Ok((ptr, elem, 0))
            }
            _ => Err(CodegenError::Msg(
                "expression is not an addressable place".into(),
            )),
        }
    }

    fn place_addr(&mut self, ptr: Value, off: i32) -> Value {
        if off == 0 {
            ptr
        } else {
            let off = self.bcx.ins().iconst(self.ptr_ty, i64::from(off));
            self.bcx.ins().iadd(ptr, off)
        }
    }

    fn read_place(&mut self, ptr: Value, ty: &Ty, off: i32) -> Result<Value, CodegenError> {
        if ty.is_aggregate() {
            Ok(self.place_addr(ptr, off))
        } else {
            self.load_mem(ptr, off, ty)
        }
    }

    fn write_place(
        &mut self,
        ptr: Value,
        ty: &Ty,
        off: i32,
        val: Value,
    ) -> Result<(), CodegenError> {
        if ty.is_aggregate() {
            let dst = self.place_addr(ptr, off);
            self.memcpy(dst, val, ty.size())
        } else {
            self.store_mem(ptr, off, ty, val)
        }
    }

    fn initialize(&mut self, dst: Value, ty: &Ty, init: &Expr) -> Result<(), CodegenError> {
        match (ty, &init.kind) {
            (Ty::Array(elem, count), ExprKind::InitList(values)) => {
                let limit = count.map_or(values.len(), |n| n.max(0) as usize);
                for (index, value) in values.iter().take(limit).enumerate() {
                    let offset = elem.size().checked_mul(index as i64).ok_or_else(|| {
                        CodegenError::Msg("array initializer offset overflow".into())
                    })?;
                    let offset = i32::try_from(offset)
                        .map_err(|_| CodegenError::Msg("array initializer is too large".into()))?;
                    let item_dst = self.place_addr(dst, offset);
                    self.initialize(item_dst, elem, value)?;
                }
                Ok(())
            }
            (Ty::Class { name, .. }, ExprKind::InitList(values)) => {
                let members = self
                    .sema
                    .classes
                    .get(name)
                    .map(|class| class.members.clone())
                    .ok_or_else(|| CodegenError::Msg(format!("unknown class `{name}`")))?;
                for (member, value) in members.iter().zip(values) {
                    let offset = i32::try_from(member.offset)
                        .map_err(|_| CodegenError::Msg("class initializer is too large".into()))?;
                    let member_dst = self.place_addr(dst, offset);
                    self.initialize(member_dst, &member.ty, value)?;
                }
                Ok(())
            }
            (_, ExprKind::InitList(values)) => {
                if let Some(value) = values.first() {
                    self.initialize(dst, ty, value)
                } else {
                    Ok(())
                }
            }
            _ if ty.is_aggregate() => {
                let src = self.expr(init)?;
                self.memcpy(dst, src, ty.size())
            }
            _ => {
                let value = self.expr(init)?;
                self.store_mem(dst, 0, ty, value)
            }
        }
    }

    fn load_mem(&mut self, ptr: Value, off: i32, ty: &Ty) -> Result<Value, CodegenError> {
        let mt = mem_clif(ty);
        let v = self.bcx.ins().load(mt, MemFlagsData::trusted(), ptr, off);
        if mt == types::I64 || mt == types::F64 {
            Ok(v)
        } else if ty.is_unsigned() {
            Ok(self.bcx.ins().uextend(types::I64, v))
        } else {
            Ok(self.bcx.ins().sextend(types::I64, v))
        }
    }

    fn store_mem(&mut self, ptr: Value, off: i32, ty: &Ty, val: Value) -> Result<(), CodegenError> {
        let mt = mem_clif(ty);
        let val = if mt == types::F64 || self.value_ty(val) == types::F64 {
            self.coerce_to(val, mt)
        } else {
            val
        };
        let v = if mt == types::I64 || mt == types::F64 {
            val
        } else {
            self.bcx.ins().ireduce(mt, val)
        };
        self.bcx.ins().store(MemFlagsData::trusted(), v, ptr, off);
        Ok(())
    }

    fn memset(&mut self, ptr: Value, val: i64, n: i64) -> Result<(), CodegenError> {
        let id = *self
            .func_ids
            .get("MemSet")
            .ok_or_else(|| CodegenError::Msg("MemSet missing".into()))?;
        let local = self.module.declare_func_in_func(id, self.bcx.func);
        let v = self.bcx.ins().iconst(types::I64, val);
        let nv = self.bcx.ins().iconst(types::I64, n);
        self.bcx.ins().call(local, &[ptr, v, nv]);
        self.after_call()?;
        Ok(())
    }

    fn memcpy(&mut self, dst: Value, src: Value, n: i64) -> Result<(), CodegenError> {
        let id = *self
            .func_ids
            .get("MemCpy")
            .ok_or_else(|| CodegenError::Msg("MemCpy missing".into()))?;
        let local = self.module.declare_func_in_func(id, self.bcx.func);
        let nv = self.bcx.ins().iconst(types::I64, n);
        self.bcx.ins().call(local, &[dst, src, nv]);
        self.after_call()?;
        Ok(())
    }

    fn finish_returns(&mut self) {
        if self.bcx.is_unreachable() {
            return;
        }
        let _ = self.pop_env();
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
            Stmt::Empty { .. } | Stmt::NoWarn { .. } | Stmt::Fn(_) => Ok(()),
            Stmt::Label { name, .. } => {
                let block = *self
                    .labels
                    .get(name)
                    .ok_or_else(|| CodegenError::Msg(format!("unknown label `{name}`")))?;
                if !self.bcx.is_unreachable() {
                    self.bcx.ins().jump(block, &[]);
                }
                self.bcx.switch_to_block(block);
                Ok(())
            }
            Stmt::Goto { label, .. } => {
                let block = *self
                    .labels
                    .get(label)
                    .ok_or_else(|| CodegenError::Msg(format!("unknown label `{label}`")))?;
                self.bcx.ins().jump(block, &[]);
                let dead = self.bcx.create_block();
                self.bcx.switch_to_block(dead);
                Ok(())
            }
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
                if self.module_scope {
                    let info = self.globals.get(&v.name).cloned().ok_or_else(|| {
                        CodegenError::Msg(format!("global `{}` was not declared", v.name))
                    })?;
                    let global = self.module.declare_data_in_func(info.id, self.bcx.func);
                    let ptr = self.bcx.ins().symbol_value(self.ptr_ty, global);
                    if let Some(init) = &v.init {
                        let ptr = self.place_addr(ptr, info.offset);
                        self.initialize(ptr, &info.ty, init)?;
                    }
                    return Ok(());
                }
                if ty.is_aggregate() {
                    if let Some(offset) = self.env_offsets.get(&v.name).copied() {
                        let env = self.own_env_ptr()?;
                        let ptr = self.place_addr(env, offset);
                        self.memset(ptr, 0, ty.size())?;
                        if let Some(init) = &v.init {
                            self.initialize(ptr, &ty, init)?;
                        }
                        self.vars
                            .insert(v.name.clone(), (LocalStorage::Env(offset), ty));
                        return Ok(());
                    }
                    let sz = ty.size().max(8) as u32;
                    let slot = self.bcx.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        sz,
                        3,
                    ));
                    let ptr = self.bcx.ins().stack_addr(self.ptr_ty, slot, 0);
                    self.memset(ptr, 0, ty.size())?;
                    if let Some(init) = &v.init {
                        self.initialize(ptr, &ty, init)?;
                    }
                    let var = self.bcx.declare_var(self.ptr_ty);
                    self.bcx.def_var(var, ptr);
                    self.vars
                        .insert(v.name.clone(), (LocalStorage::Value(var), ty));
                    return Ok(());
                }
                let cty = clif_ty(&ty, self.ptr_ty);
                let init = if let Some(e) = &v.init {
                    self.expr(e)?
                } else {
                    self.zero(cty)
                };
                self.define_local(v.name.clone(), ty, init)
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
                self.break_targets.push(exit);
                self.stmt(body)?;
                self.break_targets.pop();
                if !self.bcx.is_unreachable() {
                    self.bcx.ins().jump(header, &[]);
                }
                self.bcx.seal_block(header);
                self.bcx.seal_block(body_b);
                self.bcx.switch_to_block(exit);
                self.bcx.seal_block(exit);
                Ok(())
            }
            Stmt::DoWhile { body, cond, .. } => {
                let body_block = self.bcx.create_block();
                let condition_block = self.bcx.create_block();
                let exit = self.bcx.create_block();
                self.bcx.ins().jump(body_block, &[]);

                self.bcx.switch_to_block(body_block);
                self.break_targets.push(exit);
                self.stmt(body)?;
                self.break_targets.pop();
                if !self.bcx.is_unreachable() {
                    self.bcx.ins().jump(condition_block, &[]);
                }

                self.bcx.switch_to_block(condition_block);
                self.bcx.seal_block(condition_block);
                let condition = self.expr(cond)?;
                let condition = self.truthy(condition)?;
                self.bcx.ins().brif(condition, body_block, &[], exit, &[]);
                self.bcx.seal_block(body_block);

                self.bcx.switch_to_block(exit);
                self.bcx.seal_block(exit);
                Ok(())
            }
            Stmt::For {
                init,
                cond,
                inc,
                body,
                ..
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
                self.break_targets.push(exit);
                self.stmt(body)?;
                self.break_targets.pop();
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
            Stmt::Switch { expr, body, .. } => {
                let switch_value = self.expr(expr)?;
                let exit = self.bcx.create_block();
                let mut flat = Vec::new();
                let mut groups = Vec::new();
                flatten_switch_body(body, &mut flat, &mut groups, None);
                let case_positions: Vec<usize> = flat
                    .iter()
                    .enumerate()
                    .filter_map(|(index, stmt)| {
                        matches!(stmt.stmt, Stmt::Case { .. }).then_some(index)
                    })
                    .collect();
                if case_positions.is_empty() {
                    self.bcx.ins().jump(exit, &[]);
                    self.bcx.switch_to_block(exit);
                    self.bcx.seal_block(exit);
                    return Ok(());
                }
                let case_blocks: Vec<Block> = case_positions
                    .iter()
                    .map(|_| self.bcx.create_block())
                    .collect();
                let entry_blocks: Vec<Block> = case_positions
                    .iter()
                    .enumerate()
                    .map(|(index, pos)| {
                        if flat[*pos].group.is_some() {
                            self.bcx.create_block()
                        } else {
                            case_blocks[index]
                        }
                    })
                    .collect();
                let mut group_cases = vec![Vec::new(); groups.len()];
                for (case_index, pos) in case_positions.iter().enumerate() {
                    if let Some(group) = flat[*pos].group {
                        group_cases[group].push(case_index);
                    }
                }
                let group_tails: Vec<Option<Block>> = group_cases
                    .iter()
                    .map(|cases| (!cases.is_empty()).then(|| self.bcx.create_block()))
                    .collect();
                let default = case_positions.iter().enumerate().find_map(|(i, pos)| {
                    matches!(flat[*pos].stmt, Stmt::Case { value: None, .. })
                        .then_some(entry_blocks[i])
                });

                for (case_index, pos) in case_positions.iter().enumerate() {
                    let Stmt::Case {
                        value, range_end, ..
                    } = flat[*pos].stmt
                    else {
                        unreachable!()
                    };
                    let Some(value) = value else { continue };
                    let low = self.expr(value)?;
                    let low = self.coerce_to(low, self.value_ty(switch_value));
                    let condition = if let Some(end) = range_end {
                        let high = self.expr(end)?;
                        let high = self.coerce_to(high, self.value_ty(switch_value));
                        let ge =
                            self.bcx
                                .ins()
                                .icmp(IntCC::SignedGreaterThanOrEqual, switch_value, low);
                        let le =
                            self.bcx
                                .ins()
                                .icmp(IntCC::SignedLessThanOrEqual, switch_value, high);
                        self.bcx.ins().band(ge, le)
                    } else {
                        self.bcx.ins().icmp(IntCC::Equal, switch_value, low)
                    };
                    let next_test = self.bcx.create_block();
                    self.bcx
                        .ins()
                        .brif(condition, entry_blocks[case_index], &[], next_test, &[]);
                    self.bcx.switch_to_block(next_test);
                    self.bcx.seal_block(next_test);
                }
                self.bcx.ins().jump(default.unwrap_or(exit), &[]);

                // A HolyC `start:` group is a nested switch over the same
                // expression. Its front porch runs before every grouped case.
                for (case_index, pos) in case_positions.iter().enumerate() {
                    let Some(group) = flat[*pos].group else {
                        continue;
                    };
                    self.bcx.switch_to_block(entry_blocks[case_index]);
                    self.break_targets.push(group_tails[group].unwrap());
                    let first_case = case_positions[group_cases[group][0]];
                    for item in &flat[groups[group].start..first_case] {
                        self.stmt(item.stmt)?;
                    }
                    self.break_targets.pop();
                    if !self.bcx.is_unreachable() {
                        self.bcx.ins().jump(case_blocks[case_index], &[]);
                    }
                }

                for (case_index, pos) in case_positions.iter().enumerate() {
                    self.bcx.switch_to_block(case_blocks[case_index]);
                    let group = flat[*pos].group;
                    let next_case = case_positions.get(case_index + 1).copied();
                    let end = match group {
                        Some(group) => next_case
                            .filter(|next| flat[*next].group == Some(group))
                            .unwrap_or(groups[group].end),
                        None => next_case.unwrap_or(flat.len()),
                    };
                    self.break_targets
                        .push(group.and_then(|group| group_tails[group]).unwrap_or(exit));
                    for item in &flat[pos + 1..end] {
                        self.stmt(item.stmt)?;
                    }
                    self.break_targets.pop();
                    if !self.bcx.is_unreachable() {
                        let next = if let Some(group) = group {
                            next_case
                                .filter(|next| flat[*next].group == Some(group))
                                .map(|_| case_blocks[case_index + 1])
                                .unwrap_or(group_tails[group].unwrap())
                        } else {
                            case_blocks.get(case_index + 1).copied().unwrap_or(exit)
                        };
                        self.bcx.ins().jump(next, &[]);
                    }
                }

                // `break` inside a grouped case lands after `end:` so the
                // shared back porch executes before leaving the outer switch.
                for (group, tail) in group_tails.iter().enumerate() {
                    let Some(tail) = tail else { continue };
                    self.bcx.switch_to_block(*tail);
                    let start = groups[group].end;
                    let end = (start..flat.len())
                        .find(|index| {
                            matches!(flat[*index].stmt, Stmt::Case { .. })
                                || flat[*index].group.is_some()
                        })
                        .unwrap_or(flat.len());
                    self.break_targets.push(exit);
                    for item in &flat[start..end] {
                        self.stmt(item.stmt)?;
                    }
                    self.break_targets.pop();
                    if !self.bcx.is_unreachable() {
                        let next = case_positions
                            .iter()
                            .position(|position| *position >= end)
                            .map(|index| entry_blocks[index])
                            .unwrap_or(exit);
                        self.bcx.ins().jump(next, &[]);
                    }
                }
                for block in &case_blocks {
                    self.bcx.seal_block(*block);
                }
                for (entry, body) in entry_blocks.iter().zip(&case_blocks) {
                    if entry != body {
                        self.bcx.seal_block(*entry);
                    }
                }
                for block in group_tails.into_iter().flatten() {
                    self.bcx.seal_block(block);
                }
                self.bcx.switch_to_block(exit);
                self.bcx.seal_block(exit);
                Ok(())
            }
            Stmt::Return { expr, .. } => {
                match (expr, self.ret_ty) {
                    (Some(e), Some(_)) => {
                        let v = self.expr(e)?;
                        let v = self.coerce_to(v, self.ret_ty.unwrap());
                        self.pop_env()?;
                        self.bcx.ins().return_(&[v]);
                    }
                    (Some(e), None) => {
                        let _ = self.expr(e)?;
                        self.pop_env()?;
                        self.bcx.ins().return_(&[]);
                    }
                    (None, Some(ty)) => {
                        let z = self.zero(ty);
                        self.pop_env()?;
                        self.bcx.ins().return_(&[z]);
                    }
                    (None, None) => {
                        self.pop_env()?;
                        self.bcx.ins().return_(&[]);
                    }
                }
                let dead = self.bcx.create_block();
                self.bcx.switch_to_block(dead);
                Ok(())
            }
            Stmt::Break { .. } => {
                let target = self.break_targets.last().copied().ok_or_else(|| {
                    CodegenError::Msg("break used outside a loop or switch".into())
                })?;
                self.bcx.ins().jump(target, &[]);
                let dead = self.bcx.create_block();
                self.bcx.switch_to_block(dead);
                Ok(())
            }
            Stmt::Start { body, .. } => {
                for stmt in body {
                    if !matches!(stmt, Stmt::Case { .. }) {
                        self.stmt(stmt)?;
                    }
                }
                Ok(())
            }
            Stmt::Try { body, catch, .. } => {
                let catch_block = self.bcx.create_block();
                let join = self.bcx.create_block();
                let body_block = self.bcx.create_block();
                // Give `catch` a predecessor even if the body never throws.
                let never = self.bcx.ins().iconst(types::I8, 0);
                self.bcx
                    .ins()
                    .brif(never, catch_block, &[], body_block, &[]);
                self.bcx.switch_to_block(body_block);
                self.bcx.seal_block(body_block);
                self.catch_targets.push(catch_block);
                self.stmt(body)?;
                self.catch_targets.pop();
                if !self.bcx.is_unreachable() {
                    self.bcx.ins().jump(join, &[]);
                }
                self.bcx.switch_to_block(catch_block);
                let _ = self.call_import("tos_ClearExcept", &[])?;
                self.stmt(catch)?;
                if !self.bcx.is_unreachable() {
                    self.bcx.ins().jump(join, &[]);
                }
                self.bcx.switch_to_block(join);
                Ok(())
            }
            Stmt::Throw { expr, .. } => {
                let value = self.expr(expr)?;
                let _ = self.call_import("tos_Throw", &[value])?;
                if let Some(catch) = self.catch_targets.last().copied() {
                    self.bcx.ins().jump(catch, &[]);
                } else {
                    self.emit_unwind_return();
                }
                let dead = self.bcx.create_block();
                self.bcx.switch_to_block(dead);
                Ok(())
            }
            other => Err(CodegenError::Msg(format!(
                "statement not implemented: {other:?}"
            ))),
        }
    }

    fn truthy(&mut self, v: Value) -> Result<Value, CodegenError> {
        if self.value_ty(v) == types::F64 {
            let zero = self.bcx.ins().f64const(Ieee64::with_float(0.0));
            Ok(self.bcx.ins().fcmp(FloatCC::NotEqual, v, zero))
        } else {
            Ok(self.bcx.ins().icmp_imm_s(IntCC::NotEqual, v, 0))
        }
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
            let id = self
                .module
                .declare_data(&name, Linkage::Local, false, false)?;
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
            ExprKind::InitList(_) => Err(CodegenError::Msg(
                "initializer list used outside a declaration".into(),
            )),
            ExprKind::Ident(name) => {
                if self.vars.contains_key(name) || self.globals.contains_key(name) {
                    let (ptr, ty, off) = self.place(expr)?;
                    self.read_place(ptr, &ty, off)
                } else if self.lookup_fn(name).is_some() {
                    let id = self.func_id(name)?;
                    let f = self.module.declare_func_in_func(id, self.bcx.func);
                    Ok(self.bcx.ins().func_addr(self.ptr_ty, f))
                } else {
                    Err(CodegenError::Msg(format!("unknown identifier `{name}`")))
                }
            }
            ExprKind::Unary { op, expr, .. } => {
                let v = self.expr(expr)?;
                Ok(match op {
                    UnOp::Neg if self.value_ty(v) == types::F64 => self.bcx.ins().fneg(v),
                    UnOp::Neg => self.bcx.ins().ineg(v),
                    UnOp::Not => {
                        let truth = self.truthy(v)?;
                        let z = self.bcx.ins().icmp_imm_s(IntCC::Equal, truth, 0);
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
            ExprKind::Sequence(exprs) => {
                let mut value = self.bcx.ins().iconst(types::I64, 0);
                for item in exprs {
                    value = self.expr(item)?;
                }
                Ok(value)
            }
            ExprKind::Call { callee, args } => self.call(callee, args),
            ExprKind::Addr(inner) => {
                if let ExprKind::Ident(name) = &inner.kind
                    && self.lookup_fn(name).is_some()
                {
                    let id = self.func_id(name)?;
                    let f = self.module.declare_func_in_func(id, self.bcx.func);
                    return Ok(self.bcx.ins().func_addr(self.ptr_ty, f));
                }
                let (ptr, _, off) = self.place(inner)?;
                Ok(self.place_addr(ptr, off))
            }
            ExprKind::Deref(_) | ExprKind::Index { .. } | ExprKind::Field { .. } => {
                let (ptr, ty, off) = self.place(expr)?;
                self.read_place(ptr, &ty, off)
            }
            ExprKind::Cast { expr, .. } => self.expr(expr),
            ExprKind::Sizeof(ty) => {
                let t = resolve_ty(ty, &self.sema.classes);
                Ok(self.bcx.ins().iconst(types::I64, t.size()))
            }
            ExprKind::Offset { class, member } => {
                let offset = self
                    .sema
                    .classes
                    .get(class)
                    .and_then(|info| info.member(member))
                    .map(|info| info.offset)
                    .ok_or_else(|| {
                        CodegenError::Msg(format!("unknown class member `{class}.{member}`"))
                    })?;
                Ok(self.bcx.ins().iconst(types::I64, offset))
            }
            ExprKind::InsBin(idx) => {
                if let Some(id) = self.bins.get(&(expr.span.file, *idx)) {
                    let global = self.module.declare_data_in_func(*id, self.bcx.func);
                    Ok(self.bcx.ins().symbol_value(self.ptr_ty, global))
                } else {
                    // Keep unresolved external DolDoc references non-null so
                    // callers can still use the visible fallback renderer.
                    Ok(self.bcx.ins().iconst(self.ptr_ty, idx.saturating_add(1)))
                }
            }
            ExprKind::DollarDollar => Ok(self.bcx.ins().iconst(types::I64, 0)),
        }
    }

    fn incdec(&mut self, op: UnOp, expr: &Expr) -> Result<Value, CodegenError> {
        if matches!(
            expr.kind,
            ExprKind::Ident(_)
                | ExprKind::Field { .. }
                | ExprKind::Deref(_)
                | ExprKind::Index { .. }
        ) {
            let (ptr, ty, off) = self.place(expr)?;
            if ty.is_aggregate() {
                return Err(CodegenError::Msg("cannot increment aggregate value".into()));
            }
            let cur = self.load_mem(ptr, off, &ty)?;
            let one = if ty.is_float() {
                self.bcx.ins().f64const(Ieee64::with_float(1.0))
            } else {
                let stride = match &ty {
                    Ty::Ptr(elem) => elem.size().max(1),
                    _ => 1,
                };
                self.bcx.ins().iconst(types::I64, stride)
            };
            let nxt = match (op, ty.is_float()) {
                (UnOp::PreInc | UnOp::PostInc, true) => self.bcx.ins().fadd(cur, one),
                (UnOp::PreDec | UnOp::PostDec, true) => self.bcx.ins().fsub(cur, one),
                (UnOp::PreInc | UnOp::PostInc, false) => self.bcx.ins().iadd(cur, one),
                _ => self.bcx.ins().isub(cur, one),
            };
            self.write_place(ptr, &ty, off, nxt)?;
            return Ok(match op {
                UnOp::PostInc | UnOp::PostDec => cur,
                _ => nxt,
            });
        }
        Err(CodegenError::Msg("++/-- target is not addressable".into()))
    }

    fn cmp(&mut self, op: BinOp, l: Value, r: Value) -> Result<Value, CodegenError> {
        let (l, r, float) = self.promote_numeric(l, r);
        if float {
            let cc = match op {
                BinOp::Eq => FloatCC::Equal,
                BinOp::Ne => FloatCC::NotEqual,
                BinOp::Lt => FloatCC::LessThan,
                BinOp::Le => FloatCC::LessThanOrEqual,
                BinOp::Gt => FloatCC::GreaterThan,
                BinOp::Ge => FloatCC::GreaterThanOrEqual,
                _ => return Err(CodegenError::Msg("not a comparison".into())),
            };
            let b = self.bcx.ins().fcmp(cc, l, r);
            return Ok(self.bcx.ins().uextend(types::I64, b));
        }
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
        if matches!(op, BinOp::And | BinOp::Or) {
            return self.short_circuit(op, lhs, rhs);
        }
        let lhs_ty = self.expr_ty(lhs);
        let rhs_ty = self.expr_ty(rhs);
        let l = self.expr(lhs)?;
        let r = self.expr(rhs)?;
        if op.is_cmp() {
            return self.cmp(op, l, r);
        }
        match (&lhs_ty, &rhs_ty, op) {
            (Some(Ty::Ptr(elem)), Some(Ty::Ptr(_)), BinOp::Sub) => {
                let bytes = self.bcx.ins().isub(l, r);
                let stride = self.bcx.ins().iconst(types::I64, elem.size().max(1));
                return Ok(self.bcx.ins().sdiv(bytes, stride));
            }
            (Some(Ty::Ptr(elem)), _, BinOp::Add | BinOp::Sub) => {
                let scaled = self.bcx.ins().imul_imm_s(r, elem.size().max(1));
                return Ok(if op == BinOp::Add {
                    self.bcx.ins().iadd(l, scaled)
                } else {
                    self.bcx.ins().isub(l, scaled)
                });
            }
            (_, Some(Ty::Ptr(elem)), BinOp::Add) => {
                let scaled = self.bcx.ins().imul_imm_s(l, elem.size().max(1));
                return Ok(self.bcx.ins().iadd(scaled, r));
            }
            _ => {}
        }
        let (l, r, float) = self.promote_numeric(l, r);
        if float {
            return Ok(match op {
                BinOp::Add => self.bcx.ins().fadd(l, r),
                BinOp::Sub => self.bcx.ins().fsub(l, r),
                BinOp::Mul => self.bcx.ins().fmul(l, r),
                BinOp::Div => self.bcx.ins().fdiv(l, r),
                BinOp::Power => {
                    let results = self.call_import("tos_PowF64", &[l, r])?;
                    self.after_call()?;
                    results[0]
                }
                BinOp::And | BinOp::Or | BinOp::XorBool => {
                    let a = self.truthy(l)?;
                    let b = self.truthy(r)?;
                    let value = match op {
                        BinOp::And => self.bcx.ins().band(a, b),
                        BinOp::Or => self.bcx.ins().bor(a, b),
                        _ => self.bcx.ins().bxor(a, b),
                    };
                    self.bcx.ins().uextend(types::I64, value)
                }
                _ => {
                    return Err(CodegenError::Msg(format!(
                        "operator {op:?} is invalid for F64"
                    )));
                }
            });
        }
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
                let results = self.call_import("tos_PowI64", &[l, r])?;
                self.after_call()?;
                results[0]
            }
            _ => return Err(CodegenError::Msg(format!("binop {op:?}"))),
        })
    }

    fn short_circuit(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Value, CodegenError> {
        let lhs = self.expr(lhs)?;
        let lhs = self.truthy(lhs)?;
        let rhs_block = self.bcx.create_block();
        let join = self.bcx.create_block();
        self.bcx.append_block_param(join, types::I64);

        let zero = self.bcx.ins().iconst(types::I64, 0);
        let one = self.bcx.ins().iconst(types::I64, 1);
        let zero_arg = [zero.into()];
        let one_arg = [one.into()];
        match op {
            BinOp::And => self.bcx.ins().brif(lhs, rhs_block, &[], join, &zero_arg),
            BinOp::Or => self.bcx.ins().brif(lhs, join, &one_arg, rhs_block, &[]),
            _ => unreachable!(),
        };

        self.bcx.switch_to_block(rhs_block);
        self.bcx.seal_block(rhs_block);
        let rhs = self.expr(rhs)?;
        let rhs = self.truthy(rhs)?;
        let rhs = self.bcx.ins().uextend(types::I64, rhs);
        let rhs_arg = [rhs.into()];
        self.bcx.ins().jump(join, &rhs_arg);

        self.bcx.switch_to_block(join);
        self.bcx.seal_block(join);
        Ok(self.bcx.block_params(join)[0])
    }

    fn assign(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Value, CodegenError> {
        let r = self.expr(rhs)?;
        if matches!(
            lhs.kind,
            ExprKind::Ident(_)
                | ExprKind::Field { .. }
                | ExprKind::Deref(_)
                | ExprKind::Index { .. }
        ) {
            let (ptr, ty, off) = self.place(lhs)?;
            if ty.is_aggregate() && op != BinOp::Assign {
                return Err(CodegenError::Msg(
                    "compound assignment is invalid for aggregates".into(),
                ));
            }
            let r = if ty.is_aggregate() {
                r
            } else {
                self.coerce_to(r, clif_ty(&ty, self.ptr_ty))
            };
            let val = if op == BinOp::Assign {
                r
            } else {
                let cur = self.load_mem(ptr, off, &ty)?;
                let r = if matches!(op, BinOp::AddEq | BinOp::SubEq) {
                    if let Ty::Ptr(elem) = &ty {
                        self.bcx.ins().imul_imm_s(r, elem.size().max(1))
                    } else {
                        r
                    }
                } else {
                    r
                };
                match (op, ty.is_float()) {
                    (BinOp::AddEq, true) => self.bcx.ins().fadd(cur, r),
                    (BinOp::SubEq, true) => self.bcx.ins().fsub(cur, r),
                    (BinOp::MulEq, true) => self.bcx.ins().fmul(cur, r),
                    (BinOp::DivEq, true) => self.bcx.ins().fdiv(cur, r),
                    (BinOp::AddEq, false) => self.bcx.ins().iadd(cur, r),
                    (BinOp::SubEq, false) => self.bcx.ins().isub(cur, r),
                    (BinOp::MulEq, false) => self.bcx.ins().imul(cur, r),
                    (BinOp::DivEq, false) => self.bcx.ins().sdiv(cur, r),
                    (BinOp::ModEq, false) => self.bcx.ins().srem(cur, r),
                    (BinOp::AndEq, false) => self.bcx.ins().band(cur, r),
                    (BinOp::OrEq, false) => self.bcx.ins().bor(cur, r),
                    (BinOp::XorEq, false) => self.bcx.ins().bxor(cur, r),
                    (BinOp::ShlEq, false) => self.bcx.ins().ishl(cur, r),
                    (BinOp::ShrEq, false) => self.bcx.ins().sshr(cur, r),
                    _ => r,
                }
            };
            self.write_place(ptr, &ty, off, val)?;
            return Ok(val);
        }
        Err(CodegenError::Msg(
            "assignment target is not addressable".into(),
        ))
    }

    fn callee_function<'c>(&'c self, callee: &'c Expr) -> Option<&'c FunctionInfo> {
        let mut expr = callee;
        if let ExprKind::Deref(inner) = &expr.kind {
            expr = inner;
        }
        if let ExprKind::Addr(inner) = &expr.kind {
            expr = inner;
        }
        match &expr.kind {
            ExprKind::Ident(name) => self.lookup_fn(name),
            _ => None,
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Option<Expr>]) -> Result<Value, CodegenError> {
        let ExprKind::Ident(name) = &callee.kind else {
            let address = if let ExprKind::Deref(inner) = &callee.kind {
                self.expr(inner)?
            } else {
                self.expr(callee)?
            };
            let info = self.callee_function(callee).cloned();
            let mut values = Vec::new();
            let mut signature = self.module.make_signature();
            if let Some(info) = &info {
                for (_, ty) in &info.params {
                    signature
                        .params
                        .push(AbiParam::new(clif_ty(ty, self.ptr_ty)));
                }
                if !info.ret.is_void() {
                    signature
                        .returns
                        .push(AbiParam::new(clif_ty(&info.ret, self.ptr_ty)));
                }
            }
            for (index, arg) in args.iter().enumerate() {
                let value = match arg {
                    Some(expr) => self.expr(expr)?,
                    None => self.bcx.ins().iconst(types::I64, 0),
                };
                let value = info
                    .as_ref()
                    .and_then(|info| info.params.get(index))
                    .map_or(value, |(_, ty)| {
                        self.coerce_to(value, clif_ty(ty, self.ptr_ty))
                    });
                if info.is_none() {
                    signature.params.push(AbiParam::new(self.value_ty(value)));
                }
                values.push(value);
            }
            if info.is_none() && values.len() != signature.params.len() {
                // Keep the previous inferred-argument signature.
            }
            let signature = self.bcx.import_signature(signature);
            let call = self.bcx.ins().call_indirect(signature, address, &values);
            let results = self.bcx.inst_results(call);
            let value = if results.is_empty() {
                self.bcx.ins().iconst(types::I64, 0)
            } else {
                results[0]
            };
            self.after_call()?;
            return Ok(value);
        };
        if name == "Print" {
            return self.call_print(args);
        }
        if name == "GrPrint" {
            return self.call_gr_print(args);
        }
        let id = self.func_id(name)?;
        let params = self
            .lookup_fn(name)
            .map(|info| info.params.clone())
            .unwrap_or_default();
        let local = self.module.declare_func_in_func(id, self.bcx.func);
        let mut vals = Vec::new();
        for (index, a) in args.iter().enumerate() {
            if let Some(e) = a {
                let value = self.expr(e)?;
                let value = params.get(index).map_or(value, |(_, ty)| {
                    self.coerce_to(value, clif_ty(ty, self.ptr_ty))
                });
                vals.push(value);
            } else {
                let ty = params
                    .get(index)
                    .map(|(_, ty)| ty.clone())
                    .unwrap_or(Ty::I64);
                vals.push(self.missing_arg(name, index, &ty));
            }
        }
        for (index, (_, ty)) in params.iter().enumerate().skip(args.len()) {
            vals.push(self.missing_arg(name, index, ty));
        }
        let call = self.bcx.ins().call(local, &vals);
        let results = self.bcx.inst_results(call);
        let value = if results.is_empty() {
            self.bcx.ins().iconst(types::I64, 0)
        } else {
            results[0]
        };
        self.after_call()?;
        Ok(value)
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
                self.bcx
                    .ins()
                    .stack_store(self.ptr_ty, v, slot, (i * 8) as i32);
            }
            self.bcx.ins().stack_addr(self.ptr_ty, slot, 0)
        };
        let argc_v = self.bcx.ins().iconst(types::I64, argc);
        let id = self.func_ids["Print"];
        let local = self.module.declare_func_in_func(id, self.bcx.func);
        let call = self.bcx.ins().call(local, &[fmt, argc_v, argv]);
        let value = self.bcx.inst_results(call)[0];
        self.after_call()?;
        Ok(value)
    }

    fn call_gr_print(&mut self, args: &[Option<Expr>]) -> Result<Value, CodegenError> {
        let params = self.sema.functions["GrPrint"].params.clone();
        let mut fixed = Vec::new();
        for index in 0..4 {
            let ty = clif_ty(&params[index].1, self.ptr_ty);
            let value = match args.get(index).and_then(|a| a.as_ref()) {
                Some(expr) => {
                    let raw = self.expr(expr)?;
                    self.coerce_to(raw, ty)
                }
                None => self.zero(ty),
            };
            fixed.push(value);
        }
        let extra: Vec<&Expr> = args.iter().skip(4).filter_map(|a| a.as_ref()).collect();
        let argc = extra.len() as i64;
        let argv = if extra.is_empty() {
            self.bcx.ins().iconst(self.ptr_ty, 0)
        } else {
            let slot = self.bcx.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                (extra.len() * 8) as u32,
                0,
            ));
            for (i, expr) in extra.iter().enumerate() {
                let value = self.expr(expr)?;
                self.bcx
                    .ins()
                    .stack_store(self.ptr_ty, value, slot, (i * 8) as i32);
            }
            self.bcx.ins().stack_addr(self.ptr_ty, slot, 0)
        };
        fixed.push(self.bcx.ins().iconst(types::I64, argc));
        fixed.push(argv);
        let id = self.func_ids["GrPrint"];
        let local = self.module.declare_func_in_func(id, self.bcx.func);
        let call = self.bcx.ins().call(local, &fixed);
        let value = self.bcx.inst_results(call)[0];
        self.after_call()?;
        Ok(value)
    }
}
