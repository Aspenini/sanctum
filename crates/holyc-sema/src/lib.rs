//! Name resolution and light type checking.

use holyc_ast::*;
use holyc_syntax::{Span, SyntaxError};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct FunctionInfo {
    pub name: String,
    pub ret: Ty,
    pub params: Vec<(String, Ty)>,
    pub variadic: bool,
    pub is_builtin: bool,
    pub link_name: String,
}

#[derive(Clone, Debug)]
pub struct MemberInfo {
    pub name: String,
    pub ty: Ty,
    pub offset: i64,
    pub size: i64,
}

/// Packed class layout (TempleOS: `offset = running size; size += member_size`).
#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub name: String,
    pub size: i64,
    pub union_: bool,
    pub members: Vec<MemberInfo>,
}

impl ClassInfo {
    pub fn member(&self, name: &str) -> Option<&MemberInfo> {
        self.members.iter().find(|m| m.name == name)
    }
}

pub struct Sema {
    pub functions: HashMap<String, FunctionInfo>,
    pub globals: HashMap<String, Ty>,
    pub classes: HashMap<String, ClassInfo>,
    errors: Vec<SyntaxError>,
    path: String,
    src: String,
}

impl Sema {
    pub fn new(path: &str, src: &str) -> Self {
        let mut s = Self {
            functions: HashMap::new(),
            globals: HashMap::new(),
            classes: HashMap::new(),
            errors: Vec::new(),
            path: path.into(),
            src: src.into(),
        };
        s.add_builtin("Print", Ty::U0, vec![("fmt", Ty::Ptr(Box::new(Ty::U8)))], true);
        s.add_builtin(
            "PutChars",
            Ty::U0,
            vec![("ch", Ty::I64)],
            false,
        );
        s.add_builtin("ToI64", Ty::I64, vec![("x", Ty::F64)], false);
        s.add_builtin("ToF64", Ty::F64, vec![("x", Ty::I64)], false);
        s.add_builtin("ToBool", Ty::I64, vec![("x", Ty::I64)], false);
        s.add_builtin(
            "MAlloc",
            Ty::Ptr(Box::new(Ty::U8)),
            vec![("size", Ty::I64)],
            false,
        );
        s.add_builtin(
            "CAlloc",
            Ty::Ptr(Box::new(Ty::U8)),
            vec![("size", Ty::I64)],
            false,
        );
        s.add_builtin(
            "Free",
            Ty::U0,
            vec![("ptr", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin("StrLen", Ty::I64, vec![("s", Ty::Ptr(Box::new(Ty::U8)))], false);
        s.add_builtin(
            "MemCpy",
            Ty::Ptr(Box::new(Ty::U8)),
            vec![
                ("dst", Ty::Ptr(Box::new(Ty::U8))),
                ("src", Ty::Ptr(Box::new(Ty::U8))),
                ("n", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "MemSet",
            Ty::Ptr(Box::new(Ty::U8)),
            vec![
                ("dst", Ty::Ptr(Box::new(Ty::U8))),
                ("val", Ty::I64),
                ("n", Ty::I64),
            ],
            false,
        );
        s.add_builtin(
            "MSize",
            Ty::I64,
            vec![("ptr", Ty::Ptr(Box::new(Ty::U8)))],
            false,
        );
        s.add_builtin("QueInit", Ty::U0, vec![("head", Ty::Ptr(Box::new(Ty::U8)))], false);
        s.add_builtin(
            "QueIns",
            Ty::U0,
            vec![
                ("entry", Ty::Ptr(Box::new(Ty::U8))),
                ("pred", Ty::Ptr(Box::new(Ty::U8))),
            ],
            false,
        );
        s.add_builtin("QueRem", Ty::U0, vec![("entry", Ty::Ptr(Box::new(Ty::U8)))], false);
        s.add_builtin(
            "Bt",
            Ty::I64,
            vec![("field", Ty::Ptr(Box::new(Ty::U8))), ("bit", Ty::I64)],
            false,
        );
        s.add_builtin(
            "Bts",
            Ty::I64,
            vec![("field", Ty::Ptr(Box::new(Ty::U8))), ("bit", Ty::I64)],
            false,
        );
        s.add_builtin(
            "Btr",
            Ty::I64,
            vec![("field", Ty::Ptr(Box::new(Ty::U8))), ("bit", Ty::I64)],
            false,
        );
        s.add_builtin(
            "LBts",
            Ty::I64,
            vec![("field", Ty::Ptr(Box::new(Ty::U8))), ("bit", Ty::I64)],
            false,
        );
        s.add_builtin(
            "LBtr",
            Ty::I64,
            vec![("field", Ty::Ptr(Box::new(Ty::U8))), ("bit", Ty::I64)],
            false,
        );
        s.add_builtin("tS", Ty::F64, vec![], false);
        s.add_builtin("Rand", Ty::F64, vec![], false);
        s.add_builtin("RandU16", Ty::I64, vec![], false);
        s.add_builtin("RandU32", Ty::I64, vec![], false);
        s.add_builtin("RandI16", Ty::I64, vec![], false);
        s.add_builtin("RandI64", Ty::I64, vec![], false);
        s.add_builtin("Abs", Ty::I64, vec![("x", Ty::I64)], false);
        s.add_builtin("SqrI64", Ty::I64, vec![("x", Ty::I64)], false);
        s.add_builtin(
            "ClampI64",
            Ty::I64,
            vec![("x", Ty::I64), ("lo", Ty::I64), ("hi", Ty::I64)],
            false,
        );
        s.add_builtin("Wrap", Ty::F64, vec![("a", Ty::F64)], false);
        s.add_builtin("Sleep", Ty::U0, vec![("ms", Ty::I64)], false);
        s.add_builtin("Yield", Ty::U0, vec![], false);
        s.add_builtin("Fs", Ty::Ptr(Box::new(Ty::U8)), vec![], false);
        s.add_builtin("Gs", Ty::Ptr(Box::new(Ty::U8)), vec![], false);
        s.add_builtin("mp_cnt", Ty::I64, vec![], false);
        // Minimal CTask / CCPU so `Fs->pix_width` type-checks. Offsets match tos-abi.
        s.classes.insert(
            "CTask".into(),
            ClassInfo {
                name: "CTask".into(),
                size: 32,
                union_: false,
                members: vec![
                    MemberInfo {
                        name: "addr".into(),
                        ty: Ty::Ptr(Box::new(Ty::Class {
                            name: "CTask".into(),
                            size: 32,
                        })),
                        offset: 0,
                        size: 8,
                    },
                    MemberInfo {
                        name: "pix_width".into(),
                        ty: Ty::I64,
                        offset: 8,
                        size: 8,
                    },
                    MemberInfo {
                        name: "pix_height".into(),
                        ty: Ty::I64,
                        offset: 16,
                        size: 8,
                    },
                    MemberInfo {
                        name: "draw_it".into(),
                        ty: Ty::Ptr(Box::new(Ty::U8)),
                        offset: 24,
                        size: 8,
                    },
                ],
            },
        );
        s.classes.insert(
            "CCPU".into(),
            ClassInfo {
                name: "CCPU".into(),
                size: 8,
                union_: false,
                members: vec![MemberInfo {
                    name: "num".into(),
                    ty: Ty::I64,
                    offset: 0,
                    size: 8,
                }],
            },
        );
        s.classes.insert(
            "CQue".into(),
            packed_class(
                "CQue",
                vec![
                    (
                        "next",
                        Ty::Ptr(Box::new(Ty::Class {
                            name: "CQue".into(),
                            size: 16,
                        })),
                    ),
                    (
                        "last",
                        Ty::Ptr(Box::new(Ty::Class {
                            name: "CQue".into(),
                            size: 16,
                        })),
                    ),
                ],
            ),
        );
        s.classes.insert(
            "CD3".into(),
            packed_class("CD3", vec![("x", Ty::F64), ("y", Ty::F64), ("z", Ty::F64)]),
        );
        s.classes.insert(
            "CD3I32".into(),
            packed_class(
                "CD3I32",
                vec![("x", Ty::I32), ("y", Ty::I32), ("z", Ty::I32)],
            ),
        );
        s.classes.insert(
            "CD3I64".into(),
            packed_class(
                "CD3I64",
                vec![("x", Ty::I64), ("y", Ty::I64), ("z", Ty::I64)],
            ),
        );
        s.classes.insert(
            "CDC".into(),
            packed_class(
                "CDC",
                vec![
                    ("width", Ty::I32),
                    ("height", Ty::I32),
                    ("flags", Ty::I32),
                    ("color", Ty::U32),
                    ("r", Ty::Ptr(Box::new(Ty::I64))),
                    ("x", Ty::I32),
                    ("y", Ty::I32),
                    ("z", Ty::I32),
                    ("thick", Ty::I32),
                    ("transform", Ty::Ptr(Box::new(Ty::U8))),
                    ("body", Ty::Ptr(Box::new(Ty::U8))),
                    ("depth_buf", Ty::Ptr(Box::new(Ty::I32))),
                ],
            ),
        );
        if let Some(f) = s.functions.get_mut("Fs") {
            f.ret = Ty::Ptr(Box::new(Ty::Class {
                name: "CTask".into(),
                size: 32,
            }));
        }
        if let Some(f) = s.functions.get_mut("Gs") {
            f.ret = Ty::Ptr(Box::new(Ty::Class {
                name: "CCPU".into(),
                size: 8,
            }));
        }
        s
    }

    fn add_builtin(&mut self, name: &str, ret: Ty, params: Vec<(&str, Ty)>, variadic: bool) {
        self.functions.insert(
            name.into(),
            FunctionInfo {
                name: name.into(),
                ret,
                params: params
                    .into_iter()
                    .map(|(n, t)| (n.to_string(), t))
                    .collect(),
                variadic,
                is_builtin: true,
                link_name: format!("tos_{name}"),
            },
        );
    }

    pub fn run(&mut self, module: &mut Module) -> Result<(), SyntaxError> {
        for item in &module.items {
            match item {
                Item::Fn(f) => {
                    let ret = resolve_ty(&f.ret, &self.classes);
                    let params = f
                        .params
                        .iter()
                        .map(|p| (p.name.clone(), resolve_ty(&p.ty, &self.classes)))
                        .collect();
                    self.functions.insert(
                        f.name.clone(),
                        FunctionInfo {
                            name: f.name.clone(),
                            ret,
                            params,
                            variadic: f.variadic,
                            is_builtin: false,
                            link_name: f.name.clone(),
                        },
                    );
                }
                Item::Class(c) => {
                    let info = layout_class(c, &self.classes);
                    self.classes.insert(c.name.clone(), info);
                }
                Item::Stmt(stmt) => collect_globals(stmt, &self.classes, &mut self.globals),
            }
        }
        for item in &mut module.items {
            if let Item::Stmt(stmt) = item {
                self.rewrite_stmt(stmt);
            }
            if let Item::Fn(f) = item {
                if let Some(body) = &mut f.body {
                    for s in body {
                        self.rewrite_stmt(s);
                    }
                }
            }
        }
        if let Some(e) = self.errors.pop() {
            return Err(e);
        }
        Ok(())
    }

    fn rewrite_stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Expr { expr, span } => {
                self.rewrite_expr(expr);
                // `Main;` → `Main();` when Main is a function.
                if let ExprKind::Ident(name) = &expr.kind {
                    if self.functions.contains_key(name) {
                        let callee = expr.clone();
                        expr.kind = ExprKind::Call {
                            callee: Box::new(callee),
                            args: vec![],
                        };
                    }
                }
                let _ = span;
            }
            Stmt::Block { stmts, .. } => {
                for s in stmts {
                    self.rewrite_stmt(s);
                }
            }
            Stmt::If {
                cond, then, else_, ..
            } => {
                self.rewrite_expr(cond);
                self.rewrite_stmt(then);
                if let Some(e) = else_ {
                    self.rewrite_stmt(e);
                }
            }
            Stmt::While { cond, body, .. } | Stmt::DoWhile { cond, body, .. } => {
                self.rewrite_expr(cond);
                self.rewrite_stmt(body);
            }
            Stmt::For {
                init, cond, inc, body, ..
            } => {
                if let Some(i) = init {
                    self.rewrite_stmt(i);
                }
                if let Some(c) = cond {
                    self.rewrite_expr(c);
                }
                if let Some(i) = inc {
                    self.rewrite_expr(i);
                }
                self.rewrite_stmt(body);
            }
            Stmt::Return { expr, .. } => {
                if let Some(e) = expr {
                    self.rewrite_expr(e);
                }
            }
            Stmt::Switch { expr, body, .. } => {
                self.rewrite_expr(expr);
                self.rewrite_stmt(body);
            }
            Stmt::Start { body, .. } => {
                for s in body {
                    self.rewrite_stmt(s);
                }
            }
            Stmt::Try { body, catch, .. } => {
                self.rewrite_stmt(body);
                self.rewrite_stmt(catch);
            }
            Stmt::Throw { expr, .. } => self.rewrite_expr(expr),
            Stmt::Decl(v) => {
                if let Some(init) = &mut v.init {
                    self.rewrite_expr(init);
                }
            }
            _ => {}
        }
    }

    fn rewrite_expr(&mut self, expr: &mut Expr) {
        match &mut expr.kind {
            ExprKind::Unary { expr, .. }
            | ExprKind::Deref(expr)
            | ExprKind::Cast { expr, .. } => self.rewrite_expr(expr),
            ExprKind::Addr(inner) => {
                if !matches!(&inner.kind, ExprKind::Ident(n) if self.functions.contains_key(n)) {
                    self.rewrite_expr(inner);
                }
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.rewrite_expr(lhs);
                self.rewrite_expr(rhs);
            }
            ExprKind::ChainCmp { first, rest } => {
                self.rewrite_expr(first);
                for (_, e) in rest {
                    self.rewrite_expr(e);
                }
            }
            ExprKind::Sequence(exprs) => {
                for expr in exprs {
                    self.rewrite_expr(expr);
                }
            }
            ExprKind::Call { callee, args } => {
                self.rewrite_expr(callee);
                for a in args.iter_mut().flatten() {
                    self.rewrite_expr(a);
                }
            }
            ExprKind::Index { base, index } => {
                self.rewrite_expr(base);
                self.rewrite_expr(index);
            }
            ExprKind::InitList(values) => {
                for value in values {
                    self.rewrite_expr(value);
                }
            }
            ExprKind::Field { base, .. } => self.rewrite_expr(base),
            ExprKind::Ident(name) => {
                if name == "π" || name == "pi" {
                    expr.kind = ExprKind::Float(std::f64::consts::PI);
                } else if name == "∞" || name == "inf" {
                    expr.kind = ExprKind::Float(f64::INFINITY);
                } else if name == "TRUE" || name == "ON" || name == "true" {
                    expr.kind = ExprKind::Int(1);
                } else if name == "FALSE" || name == "OFF" || name == "false" || name == "NULL" {
                    expr.kind = ExprKind::Int(0);
                } else if let Some(f) = self.functions.get(name) {
                    // `tS` / `Dir` — no-arg (or all-default) call without `()`.
                    if f.params.is_empty() {
                        let callee = expr.clone();
                        expr.kind = ExprKind::Call {
                            callee: Box::new(callee),
                            args: vec![],
                        };
                    }
                }
            }
            _ => {}
        }
    }

    pub fn err(&mut self, span: Span, msg: impl Into<String>) {
        self.errors
            .push(SyntaxError::at(&self.path, &self.src, span, msg));
    }
}

fn collect_globals(stmt: &Stmt, classes: &HashMap<String, ClassInfo>, out: &mut HashMap<String, Ty>) {
    match stmt {
        Stmt::Decl(var) => {
            out.insert(var.name.clone(), resolve_ty(&var.ty, classes));
        }
        Stmt::Block { stmts, .. } => {
            for stmt in stmts {
                collect_globals(stmt, classes, out);
            }
        }
        _ => {}
    }
}

pub fn resolve_ty(t: &TypeRef, classes: &HashMap<String, ClassInfo>) -> Ty {
    match t {
        TypeRef::Name(n) => {
            if let Some(b) = Ty::from_builtin(n) {
                b
            } else if let Some(c) = classes.get(n) {
                Ty::Class {
                    name: n.clone(),
                    size: c.size,
                }
            } else {
                // Unknown named type: treat as a class of size 0 so `Foo *` still works.
                Ty::Class {
                    name: n.clone(),
                    size: 0,
                }
            }
        }
        TypeRef::Ptr(inner) => Ty::Ptr(Box::new(resolve_ty(inner, classes))),
        TypeRef::Array(inner, n) => Ty::Array(Box::new(resolve_ty(inner, classes)), *n),
        TypeRef::Fun {
            ret,
            params,
            variadic,
        } => Ty::Fun {
            ret: Box::new(resolve_ty(ret, classes)),
            params: params.iter().map(|p| resolve_ty(p, classes)).collect(),
            variadic: *variadic,
        },
    }
}

fn layout_class(decl: &ClassDecl, classes: &HashMap<String, ClassInfo>) -> ClassInfo {
    let mut members = Vec::new();
    let mut size = 0i64;
    if let Some(base) = &decl.base {
        if let Some(b) = classes.get(base) {
            members.extend(b.members.clone());
            size = b.size;
        }
    }
    let union_base = size;
    for m in &decl.members {
        let ty = resolve_ty(&m.ty, classes);
        let sz = ty.size().max(if matches!(ty, Ty::Ptr(_) | Ty::Fun { .. }) {
            8
        } else {
            ty.size()
        });
        let offset = if decl.union_ { union_base } else { size };
        if decl.union_ {
            size = size.max(union_base + sz);
        } else {
            size = offset + sz;
        }
        members.push(MemberInfo {
            name: m.name.clone(),
            ty,
            offset,
            size: sz,
        });
    }
    ClassInfo {
        name: decl.name.clone(),
        size,
        union_: decl.union_,
        members,
    }
}

fn packed_class(name: &str, fields: Vec<(&str, Ty)>) -> ClassInfo {
    let mut offset = 0;
    let mut members = Vec::with_capacity(fields.len());
    for (field_name, ty) in fields {
        let size = ty.size();
        members.push(MemberInfo {
            name: field_name.into(),
            ty,
            offset,
            size,
        });
        offset += size;
    }
    ClassInfo {
        name: name.into(),
        size: offset,
        union_: false,
        members,
    }
}
