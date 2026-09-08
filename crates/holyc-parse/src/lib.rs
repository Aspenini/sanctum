//! Hand-written HolyC parser. Looks up type names in a live symbol table
//! the way TempleOS does (`I64 x;` vs `Foo;` as a call).

use holyc_ast::*;
use holyc_syntax::{is_keyword, Span, SyntaxError, Token, TokenKind};
use std::collections::{HashSet, VecDeque};

pub struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    path: String,
    src: &'a str,
    /// Names currently known as types (`class` / builtin).
    types: HashSet<String>,
    pending: VecDeque<Item>,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token], path: &str, src: &'a str) -> Self {
        let mut types = HashSet::new();
        for b in [
            "I0", "I8", "I8i", "I16", "I16i", "I32", "I32i", "I64", "I64i", "U0", "U8", "U8i",
            "U16", "U16i", "U32", "U32i", "U64", "U64i", "F64", "F64i", "Bool",
            "CColorROPU32", "CQue", "CD3", "CD3I32", "CD3I64", "CTask", "CCPU", "CDC",
        ] {
            types.insert(b.into());
        }
        Self {
            tokens,
            pos: 0,
            path: path.to_string(),
            src,
            types,
            pending: VecDeque::new(),
        }
    }

    pub fn parse_module(&mut self) -> Result<Module, SyntaxError> {
        let mut items = Vec::new();
        while !self.at_eof() || !self.pending.is_empty() {
            if let Some(item) = self.pending.pop_front() {
                items.push(item);
                continue;
            }
            if self.eat_kind(&TokenKind::Semicolon) {
                continue;
            }
            items.push(self.parse_item()?);
        }
        Ok(Module { items })
    }

    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn peek_n(&self, n: usize) -> &TokenKind {
        self.tokens
            .get(self.pos + n)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn span_here(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|t| t.span)
            .unwrap_or(Span::dummy())
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    fn eat_kind(&mut self, k: &TokenKind) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(k)
            && !matches!(k, TokenKind::Ident(_) | TokenKind::Str(_) | TokenKind::Int(_))
        {
            // discriminant match is too loose for Ident. Handle simple units:
        }
        if self.peek() == k {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_punct(&mut self, k: TokenKind) -> bool {
        if *self.peek() == k {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens.get(self.pos).cloned().unwrap_or_else(|| Token {
            kind: TokenKind::Eof,
            span: Span::dummy(),
        });
        if !matches!(t.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, k: TokenKind, what: &str) -> Result<Token, SyntaxError> {
        if *self.peek() == k {
            Ok(self.bump())
        } else {
            Err(self.error(format!("expected {what}")))
        }
    }

    fn expect_ident(&mut self) -> Result<(String, Span), SyntaxError> {
        match self.bump() {
            Token {
                kind: TokenKind::Ident(s),
                span,
            } => Ok((s, span)),
            t => Err(SyntaxError::at(
                &self.path,
                self.src,
                t.span,
                "expected identifier",
            )),
        }
    }

    fn error(&self, msg: impl Into<String>) -> SyntaxError {
        SyntaxError::at(&self.path, self.src, self.span_here(), msg)
    }

    fn ident_is(&self, n: usize, s: &str) -> bool {
        matches!(self.peek_n(n), TokenKind::Ident(x) if x == s)
    }

    fn is_type_name(&self, s: &str) -> bool {
        self.types.contains(s)
    }

    fn looks_like_decl(&self) -> bool {
        let mut i = 0;
        // storage / linkage prefixes
        loop {
            match self.peek_n(i) {
                TokenKind::Ident(s)
                    if matches!(
                        s.as_str(),
                        "public"
                            | "static"
                            | "extern"
                            | "intern"
                            | "import"
                            | "_extern"
                            | "_import"
                            | "reg"
                            | "noreg"
                    ) =>
                {
                    i += 1;
                }
                _ => break,
            }
        }
        match self.peek_n(i) {
            TokenKind::Ident(s) if s == "class" || s == "union" => true,
            TokenKind::Ident(s) if self.is_type_name(s) => true,
            _ => false,
        }
    }

    fn parse_item(&mut self) -> Result<Item, SyntaxError> {
        if self.ident_is(0, "class") || self.ident_is(0, "union") || self.looks_like_class() {
            return Ok(Item::Class(self.parse_class()?));
        }
        if self.looks_like_decl() {
            return self.parse_decl_item();
        }
        // expression / call at global scope
        Ok(Item::Stmt(self.parse_stmt()?))
    }

    fn looks_like_class(&self) -> bool {
        // `public class Foo` or `I64 class I64 { ... }`
        let mut i = 0;
        if self.ident_is(0, "public") {
            i = 1;
        }
        // optional whole-object type
        if matches!(self.peek_n(i), TokenKind::Ident(s) if self.is_type_name(s)) {
            i += 1;
        }
        self.ident_is(i, "class") || self.ident_is(i, "union")
    }

    fn parse_class(&mut self) -> Result<ClassDecl, SyntaxError> {
        let span0 = self.span_here();
        let mut public = false;
        if self.ident_is(0, "public") {
            self.bump();
            public = true;
        }
        let mut whole_ty = None;
        if let TokenKind::Ident(s) = self.peek() {
            if self.is_type_name(s) && (self.ident_is(1, "class") || self.ident_is(1, "union")) {
                whole_ty = Some(TypeRef::name(s.clone()));
                self.bump();
            }
        }
        let union_ = self.ident_is(0, "union");
        if !(self.ident_is(0, "class") || union_) {
            return Err(self.error("expected class or union"));
        }
        self.bump();
        let (name, _) = self.expect_ident()?;
        let mut base = None;
        if self.eat_punct(TokenKind::Colon) {
            let (b, _) = self.expect_ident()?;
            base = Some(b);
        }
        self.expect(TokenKind::LBrace, "{")?;
        let mut members = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
            if self.eat_punct(TokenKind::Semicolon) {
                continue;
            }
            // `$$ = ...` offset reset — skip as a stmt-ish thing
            if matches!(self.peek(), TokenKind::DollarDollar) {
                self.skip_until_semi();
                continue;
            }
            let ty = self.parse_type()?;
            let base_ty = without_ptrs(&ty);
            let mut declarator_ty = ty;
            loop {
                let (mname, _) = match self.peek() {
                    TokenKind::Ident(_) => self.expect_ident()?,
                    TokenKind::DollarDollar => {
                        self.bump();
                        ("$$".into(), Span::dummy())
                    }
                    _ => break,
                };
                let mut mty = declarator_ty.clone();
                while self.eat_punct(TokenKind::LBracket) {
                    let n = match self.peek() {
                        TokenKind::Int(v) => {
                            let v = *v;
                            self.bump();
                            Some(v)
                        }
                        TokenKind::RBracket => None,
                        _ => {
                            let _ = self.parse_expr()?;
                            None
                        }
                    };
                    self.expect(TokenKind::RBracket, "]")?;
                    mty = TypeRef::Array(Box::new(mty), n);
                }
                members.push(Member { name: mname, ty: mty });
                if !self.eat_punct(TokenKind::Comma) {
                    break;
                }
                declarator_ty = base_ty.clone();
                while self.eat_punct(TokenKind::Star) {
                    declarator_ty = declarator_ty.ptr();
                }
            }
            self.eat_punct(TokenKind::Semicolon);
        }
        self.expect(TokenKind::RBrace, "}")?;
        self.types.insert(name.clone());
        if !self.eat_punct(TokenKind::Semicolon) {
            let mut ty = TypeRef::name(name.clone());
            while self.eat_punct(TokenKind::Star) {
                ty = ty.ptr();
            }
            let (var_name, _) = self.expect_ident()?;
            let decls = self.parse_var_decls(span0, ty, var_name, false, public)?;
            self.pending.push_back(Item::Stmt(decls));
        }
        Ok(ClassDecl {
            span: span0,
            public,
            union_,
            name,
            whole_ty,
            base,
            members,
        })
    }

    fn skip_until_semi(&mut self) {
        while !self.at_eof() && !matches!(self.peek(), TokenKind::Semicolon | TokenKind::RBrace) {
            self.bump();
        }
        self.eat_punct(TokenKind::Semicolon);
    }

    fn parse_decl_item(&mut self) -> Result<Item, SyntaxError> {
        let span0 = self.span_here();
        let mut public = false;
        let mut static_ = false;
        let mut extern_ = false;
        while let TokenKind::Ident(s) = self.peek() {
            match s.as_str() {
                "public" => {
                    public = true;
                    self.bump();
                }
                "static" => {
                    static_ = true;
                    self.bump();
                }
                "extern" | "intern" | "import" | "_extern" | "_import" => {
                    extern_ = true;
                    self.bump();
                    // `_extern SYM` extra name
                    if matches!(self.peek(), TokenKind::Ident(_))
                        && !self.is_type_name(self.peek().ident().unwrap_or(""))
                    {
                        // could be the symbol to bind; consume if next is a type
                        if matches!(self.peek_n(1), TokenKind::Ident(s) if self.is_type_name(s)) {
                            self.bump();
                        }
                    }
                }
                "reg" | "noreg" => {
                    self.bump();
                }
                _ => break,
            }
        }
        let ret = self.parse_type()?;
        let (name, _) = self.expect_ident()?;
        if self.eat_punct(TokenKind::LParen) {
            let (params, variadic) = self.parse_param_list()?;
            self.expect(TokenKind::RParen, ")")?;
            let body = if self.eat_punct(TokenKind::LBrace) {
                let stmts = self.parse_stmt_list()?;
                self.expect(TokenKind::RBrace, "}")?;
                Some(stmts)
            } else {
                self.eat_punct(TokenKind::Semicolon);
                None
            };
            return Ok(Item::Fn(FnDecl {
                span: span0,
                public,
                name,
                ret,
                params,
                variadic,
                body: if extern_ && body.is_none() { None } else { body },
                extern_name: None,
            }));
        }
        Ok(Item::Stmt(self.parse_var_decls(
            span0, ret, name, static_, public,
        )?))
    }

    fn parse_var_decls(
        &mut self,
        span: Span,
        shared_ty: TypeRef,
        first_name: String,
        static_: bool,
        public: bool,
    ) -> Result<Stmt, SyntaxError> {
        // Pointer stars belong to each declarator (`CD3 p,*ptr`), while the
        // base type is shared across the comma-separated declaration.
        let mut decls = Vec::new();
        let mut name = first_name;
        let base_ty = without_ptrs(&shared_ty);
        let mut declarator_ty = shared_ty;
        loop {
            let mut ty = declarator_ty.clone();
            while self.eat_punct(TokenKind::LBracket) {
                let n = if let TokenKind::Int(v) = self.peek() {
                    let v = *v;
                    self.bump();
                    Some(v)
                } else if matches!(self.peek(), TokenKind::RBracket) {
                    None
                } else {
                    // Runtime-sized arrays are represented as one element for
                    // now. Consume the bound so authentic declarations such
                    // as `per_cpu[mp_cnt]` remain parseable.
                    let _ = self.parse_expr()?;
                    None
                };
                self.expect(TokenKind::RBracket, "]")?;
                ty = TypeRef::Array(Box::new(ty), n);
            }
            let init = if self.eat_punct(TokenKind::Assign) {
                Some(self.parse_initializer()?)
            } else {
                None
            };
            decls.push(Stmt::Decl(VarDecl {
                span,
                name,
                ty,
                init,
                static_,
                public,
            }));
            if !self.eat_punct(TokenKind::Comma) {
                break;
            }
            declarator_ty = base_ty.clone();
            while self.eat_punct(TokenKind::Star) {
                declarator_ty = declarator_ty.ptr();
            }
            (name, _) = self.expect_ident()?;
        }
        self.eat_punct(TokenKind::Semicolon);
        if decls.len() == 1 {
            Ok(decls.pop().unwrap())
        } else {
            Ok(Stmt::Block {
                span,
                stmts: decls,
            })
        }
    }

    fn parse_initializer(&mut self) -> Result<Expr, SyntaxError> {
        if !matches!(self.peek(), TokenKind::LBrace) {
            return self.parse_expr();
        }
        let start = self.bump().span;
        let mut values = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
            values.push(self.parse_initializer()?);
            if !self.eat_punct(TokenKind::Comma) {
                break;
            }
        }
        let end = self.expect(TokenKind::RBrace, "}")?.span;
        Ok(Expr {
            span: start.merge(end),
            kind: ExprKind::InitList(values),
        })
    }

    fn parse_param_list(&mut self) -> Result<(Vec<Param>, bool), SyntaxError> {
        let mut params = Vec::new();
        let mut variadic = false;
        if self.eat_punct(TokenKind::Ellipsis) {
            return Ok((params, true));
        }
        if matches!(self.peek(), TokenKind::RParen) {
            return Ok((params, false));
        }
        loop {
            if self.eat_punct(TokenKind::Ellipsis) {
                variadic = true;
                break;
            }
            if matches!(self.peek(), TokenKind::RParen) {
                break;
            }
            let ty = self.parse_type()?;
            let name = if let TokenKind::Ident(s) = self.peek() {
                if is_keyword(s) {
                    format!("arg{}", params.len())
                } else {
                    let (n, _) = self.expect_ident()?;
                    n
                }
            } else {
                format!("arg{}", params.len())
            };
            let default = if self.eat_punct(TokenKind::Assign) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            params.push(Param { name, ty, default });
            if !self.eat_punct(TokenKind::Comma) {
                break;
            }
        }
        Ok((params, variadic))
    }

    fn parse_type(&mut self) -> Result<TypeRef, SyntaxError> {
        let TokenKind::Ident(name) = self.peek() else {
            return Err(self.error("expected type name"));
        };
        let name = name.clone();
        self.bump();
        let mut ty = TypeRef::name(name);
        while self.eat_punct(TokenKind::Star) {
            ty = ty.ptr();
        }
        Ok(ty)
    }

    fn parse_stmt_list(&mut self) -> Result<Vec<Stmt>, SyntaxError> {
        let mut stmts = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, SyntaxError> {
        let span = self.span_here();
        if self.eat_punct(TokenKind::Semicolon) {
            return Ok(Stmt::Empty { span });
        }
        if self.eat_punct(TokenKind::LBrace) {
            let stmts = self.parse_stmt_list()?;
            self.expect(TokenKind::RBrace, "}")?;
            return Ok(Stmt::Block { span, stmts });
        }
        if self.ident_is(0, "if") {
            return self.parse_if();
        }
        if self.ident_is(0, "while") {
            self.bump();
            self.expect(TokenKind::LParen, "(")?;
            let cond = self.parse_expr()?;
            self.expect(TokenKind::RParen, ")")?;
            let body = Box::new(self.parse_stmt()?);
            return Ok(Stmt::While { span, cond, body });
        }
        if self.ident_is(0, "do") {
            self.bump();
            let body = Box::new(self.parse_stmt()?);
            if !self.ident_is(0, "while") {
                return Err(self.error("expected while after do"));
            }
            self.bump();
            self.expect(TokenKind::LParen, "(")?;
            let cond = self.parse_expr()?;
            self.expect(TokenKind::RParen, ")")?;
            self.eat_punct(TokenKind::Semicolon);
            return Ok(Stmt::DoWhile { span, body, cond });
        }
        if self.ident_is(0, "for") {
            return self.parse_for();
        }
        if self.ident_is(0, "return") {
            self.bump();
            let expr = if matches!(self.peek(), TokenKind::Semicolon) {
                None
            } else {
                Some(self.parse_expr()?)
            };
            self.eat_punct(TokenKind::Semicolon);
            return Ok(Stmt::Return { span, expr });
        }
        if self.ident_is(0, "break") {
            self.bump();
            self.eat_punct(TokenKind::Semicolon);
            return Ok(Stmt::Break { span });
        }
        if self.ident_is(0, "goto") {
            self.bump();
            let (label, _) = self.expect_ident()?;
            self.eat_punct(TokenKind::Semicolon);
            return Ok(Stmt::Goto { span, label });
        }
        if self.ident_is(0, "switch") {
            self.bump();
            let no_bound = self.eat_punct(TokenKind::LBracket);
            if no_bound {
                self.expect(TokenKind::RBracket, "]")?;
            }
            self.expect(TokenKind::LParen, "(")?;
            let expr = self.parse_expr()?;
            self.expect(TokenKind::RParen, ")")?;
            let body = Box::new(self.parse_stmt()?);
            return Ok(Stmt::Switch {
                span,
                expr,
                no_bound,
                body,
            });
        }
        if self.ident_is(0, "case") {
            self.bump();
            let mut value = None;
            let mut range_end = None;
            if !matches!(self.peek(), TokenKind::Colon) {
                value = Some(self.parse_expr()?);
                if self.eat_punct(TokenKind::DotDot) {
                    range_end = Some(self.parse_expr()?);
                }
            }
            self.expect(TokenKind::Colon, ":")?;
            return Ok(Stmt::Case {
                span,
                value,
                range_end,
            });
        }
        if self.ident_is(0, "start") {
            self.bump();
            self.expect(TokenKind::Colon, ":")?;
            let mut body = Vec::new();
            while !self.ident_is(0, "end") && !self.at_eof() {
                body.push(self.parse_stmt()?);
            }
            if self.ident_is(0, "end") {
                self.bump();
                self.eat_punct(TokenKind::Colon);
            }
            return Ok(Stmt::Start { span, body });
        }
        if self.ident_is(0, "try") {
            self.bump();
            let body = Box::new(self.parse_stmt()?);
            if !self.ident_is(0, "catch") {
                return Err(self.error("expected catch"));
            }
            self.bump();
            let catch = Box::new(self.parse_stmt()?);
            return Ok(Stmt::Try { span, body, catch });
        }
        if self.ident_is(0, "throw") {
            self.bump();
            self.expect(TokenKind::LParen, "(")?;
            let expr = self.parse_expr()?;
            self.expect(TokenKind::RParen, ")")?;
            self.eat_punct(TokenKind::Semicolon);
            return Ok(Stmt::Throw { span, expr });
        }
        if self.ident_is(0, "no_warn") {
            self.bump();
            let mut names = Vec::new();
            while let TokenKind::Ident(s) = self.peek() {
                names.push(s.clone());
                self.bump();
                self.eat_punct(TokenKind::Comma);
                if matches!(self.peek(), TokenKind::Semicolon) {
                    break;
                }
            }
            self.eat_punct(TokenKind::Semicolon);
            return Ok(Stmt::NoWarn { span, names });
        }
        // label:  ident :
        if matches!(self.peek(), TokenKind::Ident(_)) && matches!(self.peek_n(1), TokenKind::Colon)
        {
            let (name, _) = self.expect_ident()?;
            self.bump(); // :
            return Ok(Stmt::Label { span, name });
        }
        if self.looks_like_decl() {
            match self.parse_decl_item()? {
                Item::Stmt(s) => return Ok(s),
                Item::Fn(f) => {
                    // nested function — treat as not allowed; error
                    return Err(SyntaxError::at(
                        &self.path,
                        self.src,
                        f.span,
                        "nested functions are not supported yet",
                    ));
                }
                Item::Class(c) => {
                    self.types.insert(c.name.clone());
                    return Ok(Stmt::Empty { span: c.span });
                }
            }
        }

        // Bare string / char → Print / PutChars (TempleOS).
        if matches!(self.peek(), TokenKind::Str(_)) {
            let fmt = self.parse_primary()?;
            let mut args = vec![Some(fmt.clone())];
            while self.eat_punct(TokenKind::Comma) {
                args.push(Some(self.parse_expr()?));
            }
            self.eat_punct(TokenKind::Semicolon);
            let print = Expr {
                span,
                kind: ExprKind::Ident("Print".into()),
            };
            return Ok(Stmt::Expr {
                span,
                expr: Expr {
                    span,
                    kind: ExprKind::Call {
                        callee: Box::new(print),
                        args,
                    },
                },
            });
        }
        if matches!(self.peek(), TokenKind::Char(_)) {
            let ch = self.parse_primary()?;
            self.eat_punct(TokenKind::Semicolon);
            let put = Expr {
                span,
                kind: ExprKind::Ident("PutChars".into()),
            };
            return Ok(Stmt::Expr {
                span,
                expr: Expr {
                    span,
                    kind: ExprKind::Call {
                        callee: Box::new(put),
                        args: vec![Some(ch)],
                    },
                },
            });
        }

        let expr = self.parse_comma_expr()?;
        self.eat_punct(TokenKind::Semicolon);
        Ok(Stmt::Expr { span, expr })
    }

    fn parse_if(&mut self) -> Result<Stmt, SyntaxError> {
        let span = self.span_here();
        self.bump(); // if
        self.expect(TokenKind::LParen, "(")?;
        let cond = self.parse_expr()?;
        self.expect(TokenKind::RParen, ")")?;
        let then = Box::new(self.parse_stmt()?);
        let else_ = if self.ident_is(0, "else") {
            self.bump();
            Some(Box::new(self.parse_stmt()?))
        } else {
            None
        };
        Ok(Stmt::If {
            span,
            cond,
            then,
            else_,
        })
    }

    fn parse_for(&mut self) -> Result<Stmt, SyntaxError> {
        let span = self.span_here();
        self.bump();
        self.expect(TokenKind::LParen, "(")?;
        let init = if matches!(self.peek(), TokenKind::Semicolon) {
            None
        } else if self.looks_like_decl() {
            match self.parse_decl_item()? {
                Item::Stmt(s) => Some(Box::new(s)),
                _ => None,
            }
        } else {
            let e = self.parse_comma_expr()?;
            Some(Box::new(Stmt::Expr { span: e.span, expr: e }))
        };
        self.eat_punct(TokenKind::Semicolon);
        let cond = if matches!(self.peek(), TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.eat_punct(TokenKind::Semicolon);
        let inc = if matches!(self.peek(), TokenKind::RParen) {
            None
        } else {
            Some(self.parse_comma_expr()?)
        };
        self.expect(TokenKind::RParen, ")")?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::For {
            span,
            init,
            cond,
            inc,
            body,
        })
    }

    // ---- expressions: TempleOS precedence via Pratt ----

    fn parse_expr(&mut self) -> Result<Expr, SyntaxError> {
        self.parse_prec(PREC_ASSIGN)
    }

    fn parse_comma_expr(&mut self) -> Result<Expr, SyntaxError> {
        let first = self.parse_expr()?;
        if !matches!(self.peek(), TokenKind::Comma) {
            return Ok(first);
        }
        let span = first.span;
        let mut exprs = vec![first];
        while self.eat_punct(TokenKind::Comma) {
            exprs.push(self.parse_expr()?);
        }
        let end = exprs.last().unwrap().span;
        Ok(Expr {
            span: span.merge(end),
            kind: ExprKind::Sequence(exprs),
        })
    }

    fn parse_prec(&mut self, min: u8) -> Result<Expr, SyntaxError> {
        let mut lhs = self.parse_unary()?;
        loop {
            let Some(op) = BinOp::from_token(self.peek()) else {
                break;
            };
            let prec = bin_prec(op);
            if prec < min {
                break;
            }
            self.bump();
            if op.is_cmp() {
                // chained comparisons
                let rhs = self.parse_prec(prec + 1)?;
                let mut rest = vec![(op, rhs)];
                while let Some(op2) = BinOp::from_token(self.peek()) {
                    if !op2.is_cmp() || bin_prec(op2) < min {
                        break;
                    }
                    self.bump();
                    let r = self.parse_prec(prec + 1)?;
                    rest.push((op2, r));
                }
                if rest.len() == 1 {
                    let (op, rhs) = rest.pop().unwrap();
                    let span = lhs.span.merge(rhs.span);
                    lhs = Expr {
                        span,
                        kind: ExprKind::Binary {
                            op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                    };
                } else {
                    let span = lhs.span.merge(rest.last().unwrap().1.span);
                    lhs = Expr {
                        span,
                        kind: ExprKind::ChainCmp {
                            first: Box::new(lhs),
                            rest,
                        },
                    };
                }
                continue;
            }
            let right_assoc = op.is_assign() || op == BinOp::Power;
            let next_min = if right_assoc { prec } else { prec + 1 };
            let rhs = self.parse_prec(next_min)?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr {
                span,
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, SyntaxError> {
        let span = self.span_here();
        match self.peek() {
            TokenKind::Minus => {
                self.bump();
                let expr = self.parse_unary()?;
                return Ok(Expr {
                    span: span.merge(expr.span),
                    kind: ExprKind::Unary {
                        op: UnOp::Neg,
                        expr: Box::new(expr),
                        postfix: false,
                    },
                });
            }
            TokenKind::Bang => {
                self.bump();
                let expr = self.parse_unary()?;
                return Ok(Expr {
                    span: span.merge(expr.span),
                    kind: ExprKind::Unary {
                        op: UnOp::Not,
                        expr: Box::new(expr),
                        postfix: false,
                    },
                });
            }
            TokenKind::Tilde => {
                self.bump();
                let expr = self.parse_unary()?;
                return Ok(Expr {
                    span: span.merge(expr.span),
                    kind: ExprKind::Unary {
                        op: UnOp::BitNot,
                        expr: Box::new(expr),
                        postfix: false,
                    },
                });
            }
            TokenKind::PlusPlus => {
                self.bump();
                let expr = self.parse_unary()?;
                return Ok(Expr {
                    span: span.merge(expr.span),
                    kind: ExprKind::Unary {
                        op: UnOp::PreInc,
                        expr: Box::new(expr),
                        postfix: false,
                    },
                });
            }
            TokenKind::MinusMinus => {
                self.bump();
                let expr = self.parse_unary()?;
                return Ok(Expr {
                    span: span.merge(expr.span),
                    kind: ExprKind::Unary {
                        op: UnOp::PreDec,
                        expr: Box::new(expr),
                        postfix: false,
                    },
                });
            }
            TokenKind::Star => {
                self.bump();
                let expr = self.parse_unary()?;
                return Ok(Expr {
                    span: span.merge(expr.span),
                    kind: ExprKind::Deref(Box::new(expr)),
                });
            }
            TokenKind::Amp => {
                self.bump();
                let expr = self.parse_unary()?;
                return Ok(Expr {
                    span: span.merge(expr.span),
                    kind: ExprKind::Addr(Box::new(expr)),
                });
            }
            TokenKind::Plus => {
                self.bump();
                return self.parse_unary();
            }
            _ => {}
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, SyntaxError> {
        let mut expr = self.parse_primary()?;
        loop {
            let span0 = expr.span;
            match self.peek() {
                TokenKind::LParen => {
                    self.bump();
                    let args = self.parse_arg_list()?;
                    let end = self.expect(TokenKind::RParen, ")")?;
                    expr = Expr {
                        span: span0.merge(end.span),
                        kind: ExprKind::Call {
                            callee: Box::new(expr),
                            args,
                        },
                    };
                }
                TokenKind::LBracket => {
                    self.bump();
                    let index = self.parse_expr()?;
                    let end = self.expect(TokenKind::RBracket, "]")?;
                    expr = Expr {
                        span: span0.merge(end.span),
                        kind: ExprKind::Index {
                            base: Box::new(expr),
                            index: Box::new(index),
                        },
                    };
                }
                TokenKind::Dot | TokenKind::Arrow => {
                    let arrow = matches!(self.peek(), TokenKind::Arrow);
                    self.bump();
                    let (name, sp) = self.expect_ident()?;
                    expr = Expr {
                        span: span0.merge(sp),
                        kind: ExprKind::Field {
                            base: Box::new(expr),
                            name,
                            arrow,
                        },
                    };
                }
                TokenKind::PlusPlus => {
                    let t = self.bump();
                    expr = Expr {
                        span: span0.merge(t.span),
                        kind: ExprKind::Unary {
                            op: UnOp::PostInc,
                            expr: Box::new(expr),
                            postfix: true,
                        },
                    };
                }
                TokenKind::MinusMinus => {
                    let t = self.bump();
                    expr = Expr {
                        span: span0.merge(t.span),
                        kind: ExprKind::Unary {
                            op: UnOp::PostDec,
                            expr: Box::new(expr),
                            postfix: true,
                        },
                    };
                }
                // postfix cast:  expr(Type)  already handled as call.
                // TempleOS postfix cast is `i(I64)` which looks like a call if i is not a fun.
                // Sema rewrites that. Also `i.u16[1]` is field+index.
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_arg_list(&mut self) -> Result<Vec<Option<Expr>>, SyntaxError> {
        let mut args = Vec::new();
        if matches!(self.peek(), TokenKind::RParen) {
            return Ok(args);
        }
        loop {
            if matches!(self.peek(), TokenKind::Comma) {
                args.push(None);
                self.bump();
                continue;
            }
            if matches!(self.peek(), TokenKind::RParen) {
                break;
            }
            args.push(Some(self.parse_expr()?));
            if !self.eat_punct(TokenKind::Comma) {
                break;
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, SyntaxError> {
        let t = self.bump();
        let span = t.span;
        match t.kind {
            TokenKind::Int(v) => Ok(Expr {
                span,
                kind: ExprKind::Int(v),
            }),
            TokenKind::Float(v) => Ok(Expr {
                span,
                kind: ExprKind::Float(v),
            }),
            TokenKind::Str(mut s) => {
                let mut full_span = span;
                while let TokenKind::Str(next) = self.peek() {
                    s.push_str(next);
                    full_span = full_span.merge(self.bump().span);
                }
                Ok(Expr {
                    span: full_span,
                    kind: ExprKind::Str(s),
                })
            }
            TokenKind::Char(v) => Ok(Expr {
                span,
                kind: ExprKind::Char(v),
            }),
            TokenKind::Ident(s) if s == "sizeof" => {
                self.expect(TokenKind::LParen, "(")?;
                let ty = self.parse_type()?;
                self.expect(TokenKind::RParen, ")")?;
                Ok(Expr {
                    span,
                    kind: ExprKind::Sizeof(ty),
                })
            }
            TokenKind::Ident(s) if s == "offset" => {
                self.expect(TokenKind::LParen, "(")?;
                let (class, _) = self.expect_ident()?;
                self.expect(TokenKind::Dot, ".")?;
                let (member, _) = self.expect_ident()?;
                self.expect(TokenKind::RParen, ")")?;
                Ok(Expr {
                    span,
                    kind: ExprKind::Offset { class, member },
                })
            }
            TokenKind::Ident(s) => Ok(Expr {
                span,
                kind: ExprKind::Ident(s),
            }),
            TokenKind::InsBin { idx } => Ok(Expr {
                span,
                kind: ExprKind::InsBin(idx),
            }),
            TokenKind::DollarDollar => Ok(Expr {
                span,
                kind: ExprKind::DollarDollar,
            }),
            TokenKind::LParen => {
                let e = self.parse_expr()?;
                self.expect(TokenKind::RParen, ")")?;
                Ok(e)
            }
            other => Err(SyntaxError::at(
                &self.path,
                self.src,
                span,
                format!("expected expression, got {other:?}"),
            )),
        }
    }
}

fn without_ptrs(ty: &TypeRef) -> TypeRef {
    match ty {
        TypeRef::Ptr(inner) => without_ptrs(inner),
        _ => ty.clone(),
    }
}

const PREC_ASSIGN: u8 = 1;
const PREC_OR: u8 = 2;
const PREC_XOR: u8 = 3;
const PREC_AND: u8 = 4;
const PREC_EQ: u8 = 5;
const PREC_CMP: u8 = 6;
const PREC_ADD: u8 = 7;
const PREC_BOR: u8 = 8;
const PREC_BXOR: u8 = 9;
const PREC_BAND: u8 = 10;
const PREC_MUL: u8 = 11;
const PREC_SHIFT: u8 = 12;

fn bin_prec(op: BinOp) -> u8 {
    // TempleOS: ` >> <<   * / %   &   ^   |   + -   < >   == !=   &&   ^^   ||   =
    match op {
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
        | BinOp::ShrEq => PREC_ASSIGN,
        BinOp::Or => PREC_OR,
        BinOp::XorBool => PREC_XOR,
        BinOp::And => PREC_AND,
        BinOp::Eq | BinOp::Ne => PREC_EQ,
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => PREC_CMP,
        BinOp::Add | BinOp::Sub => PREC_ADD,
        BinOp::BitOr => PREC_BOR,
        BinOp::BitXor => PREC_BXOR,
        BinOp::BitAnd => PREC_BAND,
        BinOp::Mul | BinOp::Div | BinOp::Mod => PREC_MUL,
        BinOp::Shl | BinOp::Shr | BinOp::Power => PREC_SHIFT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use holyc_syntax::{lex_buffer, Session};

    fn parse(src: &str) -> Module {
        let mut sess = Session::new();
        let toks = lex_buffer(&mut sess, "t.HC", src).unwrap();
        Parser::new(&toks, "t.HC", src).parse_module().unwrap()
    }

    #[test]
    fn hello_ast() {
        let m = parse("U0 Main()\n{\n  \"Hello world\\n\";\n}\nMain;\n");
        assert_eq!(m.items.len(), 2);
        assert!(matches!(m.items[0], Item::Fn(_)));
        assert!(matches!(m.items[1], Item::Stmt(Stmt::Expr { .. })));
    }
}
