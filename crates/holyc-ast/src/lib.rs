//! HolyC AST and type representations.

use holyc_syntax::{Span, TokenKind};

#[derive(Clone, Debug)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Fn(FnDecl),
    Class(ClassDecl),
    Stmt(Stmt),
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub span: Span,
    pub public: bool,
    pub name: String,
    pub ret: TypeRef,
    pub params: Vec<Param>,
    pub variadic: bool,
    pub body: Option<Vec<Stmt>>, // None = extern/import
    pub extern_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: TypeRef,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct ClassDecl {
    pub span: Span,
    pub public: bool,
    pub union_: bool,
    pub name: String,
    pub whole_ty: Option<TypeRef>,
    pub base: Option<String>,
    pub members: Vec<Member>,
}

#[derive(Clone, Debug)]
pub struct Member {
    pub name: String,
    pub ty: TypeRef,
}

#[derive(Clone, Debug)]
pub struct VarDecl {
    pub span: Span,
    pub name: String,
    pub ty: TypeRef,
    pub init: Option<Expr>,
    pub static_: bool,
    pub public: bool,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Block {
        span: Span,
        stmts: Vec<Stmt>,
    },
    Expr {
        span: Span,
        expr: Expr,
    },
    Decl(VarDecl),
    If {
        span: Span,
        cond: Expr,
        then: Box<Stmt>,
        else_: Option<Box<Stmt>>,
    },
    While {
        span: Span,
        cond: Expr,
        body: Box<Stmt>,
    },
    DoWhile {
        span: Span,
        body: Box<Stmt>,
        cond: Expr,
    },
    For {
        span: Span,
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        inc: Option<Expr>,
        body: Box<Stmt>,
    },
    Return {
        span: Span,
        expr: Option<Expr>,
    },
    Break {
        span: Span,
    },
    Goto {
        span: Span,
        label: String,
    },
    Label {
        span: Span,
        name: String,
    },
    Switch {
        span: Span,
        expr: Expr,
        no_bound: bool,
        body: Box<Stmt>,
    },
    Case {
        span: Span,
        value: Option<Expr>,
        range_end: Option<Expr>,
    },
    Start {
        span: Span,
        body: Vec<Stmt>,
    },
    Try {
        span: Span,
        body: Box<Stmt>,
        catch: Box<Stmt>,
    },
    Throw {
        span: Span,
        expr: Expr,
    },
    NoWarn {
        span: Span,
        names: Vec<String>,
    },
    Empty {
        span: Span,
    },
    Fn(FnDecl),
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Block { span, .. }
            | Stmt::Expr { span, .. }
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::DoWhile { span, .. }
            | Stmt::For { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::Break { span }
            | Stmt::Goto { span, .. }
            | Stmt::Label { span, .. }
            | Stmt::Switch { span, .. }
            | Stmt::Case { span, .. }
            | Stmt::Start { span, .. }
            | Stmt::Try { span, .. }
            | Stmt::Throw { span, .. }
            | Stmt::NoWarn { span, .. }
            | Stmt::Empty { span } => *span,
            Stmt::Decl(d) => d.span,
            Stmt::Fn(function) => function.span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub span: Span,
    pub kind: ExprKind,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Str(String),
    Char(i64),
    InitList(Vec<Expr>),
    Ident(String),
    /// `$IB` sprite pointer.
    InsBin(i64),
    Unary {
        op: UnOp,
        expr: Box<Expr>,
        postfix: bool,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// Chained comparison: `a<b<c` → cmp(a, <, b, <, c)
    ChainCmp {
        first: Box<Expr>,
        rest: Vec<(BinOp, Expr)>,
    },
    Sequence(Vec<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<Option<Expr>>, // None = default-arg hole (`Test(,3)`)
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: String,
        arrow: bool,
    },
    Cast {
        expr: Box<Expr>,
        ty: TypeRef,
    },
    Sizeof(TypeRef),
    Offset {
        class: String,
        member: String,
    },
    Addr(Box<Expr>),
    Deref(Box<Expr>),
    /// `$$` in a class or expression.
    DollarDollar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    BitNot,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Power,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    XorBool,
    Assign,
    AddEq,
    SubEq,
    MulEq,
    DivEq,
    ModEq,
    AndEq,
    OrEq,
    XorEq,
    ShlEq,
    ShrEq,
}

impl BinOp {
    pub fn from_token(k: &TokenKind) -> Option<Self> {
        Some(match k {
            TokenKind::Plus => BinOp::Add,
            TokenKind::Minus => BinOp::Sub,
            TokenKind::Star => BinOp::Mul,
            TokenKind::Slash => BinOp::Div,
            TokenKind::Percent => BinOp::Mod,
            TokenKind::Amp => BinOp::BitAnd,
            TokenKind::Pipe => BinOp::BitOr,
            TokenKind::Caret => BinOp::BitXor,
            TokenKind::Shl => BinOp::Shl,
            TokenKind::Shr => BinOp::Shr,
            TokenKind::Power => BinOp::Power,
            TokenKind::EqEq => BinOp::Eq,
            TokenKind::NotEq => BinOp::Ne,
            TokenKind::Lt => BinOp::Lt,
            TokenKind::Le => BinOp::Le,
            TokenKind::Gt => BinOp::Gt,
            TokenKind::Ge => BinOp::Ge,
            TokenKind::AndAnd => BinOp::And,
            TokenKind::OrOr => BinOp::Or,
            TokenKind::XorXor => BinOp::XorBool,
            TokenKind::Assign => BinOp::Assign,
            TokenKind::AddEq => BinOp::AddEq,
            TokenKind::SubEq => BinOp::SubEq,
            TokenKind::MulEq => BinOp::MulEq,
            TokenKind::DivEq => BinOp::DivEq,
            TokenKind::ModEq => BinOp::ModEq,
            TokenKind::AndEq => BinOp::AndEq,
            TokenKind::OrEq => BinOp::OrEq,
            TokenKind::XorEq => BinOp::XorEq,
            TokenKind::ShlEq => BinOp::ShlEq,
            TokenKind::ShrEq => BinOp::ShrEq,
            _ => return None,
        })
    }

    pub fn is_assign(self) -> bool {
        matches!(
            self,
            BinOp::Assign
                | BinOp::AddEq
                | BinOp::SubEq
                | BinOp::MulEq
                | BinOp::DivEq
                | BinOp::ModEq
                | BinOp::AndEq
                | BinOp::OrEq
                | BinOp::XorEq
                | BinOp::ShlEq
                | BinOp::ShrEq
        )
    }

    pub fn is_cmp(self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        )
    }
}

/// Unresolved or resolved type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeRef {
    Name(String),
    Ptr(Box<TypeRef>),
    Array(Box<TypeRef>, Option<i64>),
    Fun {
        ret: Box<TypeRef>,
        params: Vec<TypeRef>,
        variadic: bool,
    },
}

impl TypeRef {
    pub fn name(s: impl Into<String>) -> Self {
        TypeRef::Name(s.into())
    }

    pub fn ptr(self) -> Self {
        TypeRef::Ptr(Box::new(self))
    }

    pub fn is_voidish(&self) -> bool {
        matches!(self, TypeRef::Name(n) if n == "U0" || n == "I0" || n == "void")
    }
}

/// After sema: concrete type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    I0,
    I8,
    I16,
    I32,
    I64,
    U0,
    U8,
    U16,
    U32,
    U64,
    F64,
    Ptr(Box<Ty>),
    Array(Box<Ty>, Option<i64>),
    Class {
        name: String,
        size: i64,
    },
    Fun {
        ret: Box<Ty>,
        params: Vec<Ty>,
        variadic: bool,
    },
}

impl Ty {
    pub fn from_builtin(name: &str) -> Option<Ty> {
        Some(match name {
            "I0" => Ty::I0,
            "I8" | "I8i" => Ty::I8,
            "I16" | "I16i" => Ty::I16,
            "I32" | "I32i" => Ty::I32,
            "I64" | "I64i" => Ty::I64,
            "U0" => Ty::U0,
            "U8" | "U8i" => Ty::U8,
            "U16" | "U16i" => Ty::U16,
            "U32" | "U32i" => Ty::U32,
            "U64" | "U64i" => Ty::U64,
            "F64" | "F64i" => Ty::F64,
            "Bool" => Ty::I64,
            "CColorROPU32" => Ty::U32,
            _ => return None,
        })
    }

    pub fn size(&self) -> i64 {
        match self {
            Ty::I0 | Ty::U0 => 0,
            Ty::I8 | Ty::U8 => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 => 4,
            Ty::I64 | Ty::U64 | Ty::F64 | Ty::Ptr(_) | Ty::Fun { .. } => 8,
            Ty::Array(elem, Some(n)) => elem.size() * n,
            Ty::Array(elem, None) => elem.size(),
            Ty::Class { size, .. } => *size,
        }
    }

    pub fn is_int(&self) -> bool {
        matches!(
            self,
            Ty::I0
                | Ty::I8
                | Ty::I16
                | Ty::I32
                | Ty::I64
                | Ty::U0
                | Ty::U8
                | Ty::U16
                | Ty::U32
                | Ty::U64
                | Ty::Ptr(_)
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Ty::F64)
    }

    pub fn is_void(&self) -> bool {
        matches!(self, Ty::U0 | Ty::I0)
    }

    pub fn is_unsigned(&self) -> bool {
        matches!(self, Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64)
    }

    pub fn is_aggregate(&self) -> bool {
        matches!(self, Ty::Class { .. } | Ty::Array(_, _))
    }

    pub fn class_name(&self) -> Option<&str> {
        match self {
            Ty::Class { name, .. } => Some(name),
            Ty::Ptr(inner) => inner.class_name(),
            _ => None,
        }
    }

    /// HolyC extends loaded values to I64.
    pub fn as_rvalue(&self) -> Ty {
        match self {
            Ty::F64 => Ty::F64,
            Ty::U0 | Ty::I0 => self.clone(),
            Ty::Class { .. } => self.clone(),
            Ty::Array(elem, _) => Ty::Ptr(elem.clone()),
            _ => Ty::I64,
        }
    }
}
