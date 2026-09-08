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

pub struct Sema {
    pub functions: HashMap<String, FunctionInfo>,
    pub globals: HashMap<String, Ty>,
    pub classes: HashMap<String, ClassDecl>,
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
                    self.classes.insert(c.name.clone(), c.clone());
                }
                Item::Stmt(Stmt::Decl(v)) => {
                    self.globals
                        .insert(v.name.clone(), resolve_ty(&v.ty, &self.classes));
                }
                _ => {}
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
            | ExprKind::Addr(expr)
            | ExprKind::Deref(expr)
            | ExprKind::Cast { expr, .. } => self.rewrite_expr(expr),
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
            ExprKind::Field { base, .. } => self.rewrite_expr(base),
            ExprKind::Ident(name) => {
                // π / inf aliases
                if name == "π" || name == "pi" {
                    expr.kind = ExprKind::Float(std::f64::consts::PI);
                } else if name == "∞" || name == "inf" {
                    expr.kind = ExprKind::Float(f64::INFINITY);
                } else if name == "TRUE" || name == "ON" || name == "true" {
                    expr.kind = ExprKind::Int(1);
                } else if name == "FALSE" || name == "OFF" || name == "false" || name == "NULL" {
                    expr.kind = ExprKind::Int(0);
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

pub fn resolve_ty(t: &TypeRef, classes: &HashMap<String, ClassDecl>) -> Ty {
    match t {
        TypeRef::Name(n) => {
            if let Some(b) = Ty::from_builtin(n) {
                b
            } else if let Some(c) = classes.get(n) {
                let size = c.members.iter().map(|m| resolve_ty(&m.ty, classes).size()).sum();
                Ty::Class {
                    name: n.clone(),
                    size,
                }
            } else {
                Ty::I64
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
