use crate::ast::*;
use crate::lexer::{SpannedToken, Token};
use std::ops::Range;

const PREFIX_BP: u8 = 17;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Range<usize>,
}

pub struct Parser<'a> {
    source: &'a str,
    tokens: &'a [SpannedToken],
    pos: usize,
    arena: &'a mut AstArena,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str, tokens: &'a [SpannedToken], arena: &'a mut AstArena) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            arena,
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|st| &st.token)
    }

    fn peek_span(&self) -> Range<usize> {
        match self.tokens.get(self.pos) {
            Some(st) => st.span.clone(),
            None => {
                let end = self.tokens.last().map(|st| st.span.end).unwrap_or(0);
                end..end
            }
        }
    }

    fn has_leading_newline(&self) -> bool {
        self.tokens
            .get(self.pos)
            .map(|st| st.leading_newline)
            .unwrap_or(false)
    }

    fn peek_ahead(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.pos + n).map(|st| &st.token)
    }

    fn advance(&mut self) -> Option<&SpannedToken> {
        if self.pos < self.tokens.len() {
            let st = &self.tokens[self.pos];
            self.pos += 1;
            Some(st)
        } else {
            None
        }
    }

    fn expect(&mut self, expected: Token) -> Result<Range<usize>, ParseError> {
        let span = self.peek_span();
        match self.peek() {
            Some(t) if t == &expected => {
                self.advance();
                Ok(span)
            }
            Some(other) => Err(ParseError {
                message: format!("Expected {:?}, found {:?}", expected, other),
                span,
            }),
            None => Err(ParseError {
                message: format!("Expected {:?}, found end of file", expected),
                span,
            }),
        }
    }

    fn expect_ident(&mut self) -> Result<(String, Range<usize>), ParseError> {
        let span = self.peek_span();
        if let Some(tok) = self.peek() {
            if let Some(name) = tok.as_ident() {
                self.advance();
                return Ok((name, span));
            }
        }
        match self.peek() {
            Some(other) => Err(ParseError {
                message: format!("Expected identifier, found {:?}", other),
                span,
            }),
            None => Err(ParseError {
                message: "Expected identifier, found end of file".to_string(),
                span,
            }),
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        while self.peek().is_some() {
            items.push(self.parse_item()?);
        }
        Ok(Program { items })
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        match self.peek() {
            Some(Token::Type) => Ok(Item::Type(self.parse_type_decl()?)),
            Some(Token::Schema) => Ok(Item::Schema(self.parse_schema_decl()?)),
            Some(Token::Route) => Ok(Item::Route(self.parse_route_decl()?)),
            Some(Token::Page) => Ok(Item::Page(self.parse_page_decl()?)),
            Some(Token::Ui) => Ok(Item::Ui(self.parse_ui_decl()?)),
            Some(Token::Fn) => Ok(Item::Fn(self.parse_fn_decl()?)),
            Some(other) => Err(ParseError {
                message: format!("Expected top-level declaration (type, schema, route, page, ui, fn), found {:?}", other),
                span: self.peek_span(),
            }),
            None => Err(ParseError {
                message: "Unexpected end of file".to_string(),
                span: self.peek_span(),
            }),
        }
    }

    // -------------------------------------------------------------
    // Type and Schema Declarations
    // -------------------------------------------------------------
    fn parse_type_decl(&mut self) -> Result<TypeDecl, ParseError> {
        self.expect(Token::Type)?;
        let (name, _) = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut fields = Vec::new();

        while self.peek() != Some(&Token::RBrace) && self.peek().is_some() {
            let (field_name, _) = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let field_ty = self.parse_type_ref()?;
            fields.push(FieldDef {
                name: field_name,
                ty: field_ty,
            });

            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(Token::RBrace)?;
        Ok(TypeDecl { name, fields })
    }

    fn parse_schema_decl(&mut self) -> Result<SchemaDecl, ParseError> {
        self.expect(Token::Schema)?;
        let (name, _) = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut fields = Vec::new();

        while self.peek() != Some(&Token::RBrace) && self.peek().is_some() {
            let (field_name, _) = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let field_ty = self.parse_type_ref()?;

            let mut modifiers = Vec::new();
            while let Some(tok) = self.peek() {
                match tok {
                    Token::Pk => {
                        self.advance();
                        modifiers.push(FieldModifier::Pk);
                    }
                    Token::Unique => {
                        self.advance();
                        modifiers.push(FieldModifier::Unique);
                    }
                    Token::Indexed => {
                        self.advance();
                        modifiers.push(FieldModifier::Indexed);
                    }
                    Token::Auto => {
                        self.advance();
                        modifiers.push(FieldModifier::Auto);
                    }
                    Token::Default => {
                        self.advance();
                        self.expect(Token::LParen)?;
                        let expr = self.parse_expr(0)?;
                        self.expect(Token::RParen)?;
                        modifiers.push(FieldModifier::Default(expr));
                    }
                    _ => break,
                }
            }

            fields.push(SchemaField {
                name: field_name,
                ty: field_ty,
                modifiers,
            });

            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(Token::RBrace)?;
        Ok(SchemaDecl { name, fields })
    }

    fn parse_type_ref(&mut self) -> Result<TypeRef, ParseError> {
        let mut base = match self.peek() {
            Some(Token::TypeInt) => {
                self.advance();
                TypeRef::Int
            }
            Some(Token::TypeFloat) => {
                self.advance();
                TypeRef::Float
            }
            Some(Token::TypeStr) => {
                self.advance();
                TypeRef::Str
            }
            Some(Token::TypeBool) => {
                self.advance();
                TypeRef::Bool
            }
            Some(Token::TypeAny) => {
                self.advance();
                TypeRef::Any
            }
            Some(Token::TypeVoid) => {
                self.advance();
                TypeRef::Void
            }
            Some(tok) if tok.as_ident().is_some() => {
                let name = tok.as_ident().unwrap();
                self.advance();
                TypeRef::Custom(name)
            }
            Some(Token::LBracket) => {
                self.advance();
                let inner = self.parse_type_ref()?;
                self.expect(Token::RBracket)?;
                TypeRef::Array(Box::new(inner))
            }
            Some(Token::LBrace) => {
                self.advance();
                let mut fields = Vec::new();
                while self.peek() != Some(&Token::RBrace) && self.peek().is_some() {
                    let (fname, _) = self.expect_ident()?;
                    self.expect(Token::Colon)?;
                    let fty = self.parse_type_ref()?;
                    fields.push(FieldDef {
                        name: fname,
                        ty: fty,
                    });
                    if self.peek() == Some(&Token::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(Token::RBrace)?;
                TypeRef::Record(fields)
            }
            Some(other) => {
                return Err(ParseError {
                    message: format!("Expected type reference, found {:?}", other),
                    span: self.peek_span(),
                })
            }
            None => {
                return Err(ParseError {
                    message: "Expected type reference, found end of file".to_string(),
                    span: self.peek_span(),
                })
            }
        };

        if self.peek() == Some(&Token::Question) {
            self.advance();
            base = TypeRef::Optional(Box::new(base));
        }

        Ok(base)
    }

    // -------------------------------------------------------------
    // Route and Page Declarations
    // -------------------------------------------------------------
    fn parse_route_decl(&mut self) -> Result<RouteDecl, ParseError> {
        self.expect(Token::Route)?;
        let path = self.parse_path()?;
        let config = self.parse_route_config()?;

        let return_type = if self.peek() == Some(&Token::Arrow) {
            self.advance();
            Some(self.parse_type_ref()?)
        } else {
            None
        };

        let body = self.parse_block()?;
        Ok(RouteDecl {
            path,
            config,
            return_type,
            body,
        })
    }

    fn parse_page_decl(&mut self) -> Result<PageDecl, ParseError> {
        self.expect(Token::Page)?;
        let path = self.parse_path()?;
        let config = self.parse_route_config()?;
        let body = self.parse_block()?;
        Ok(PageDecl { path, config, body })
    }

    fn parse_path(&mut self) -> Result<String, ParseError> {
        let start = self.expect(Token::Slash)?.start;
        let mut end = start + 1;

        while self.pos < self.tokens.len() && self.tokens[self.pos].span.start == end {
            let tok = &self.tokens[self.pos].token;
            let is_path_part = matches!(
                tok,
                Token::Slash
                    | Token::Ident(_)
                    | Token::IntLiteral(_)
                    | Token::Minus
                    | Token::LBrace
                    | Token::RBrace
                    | Token::Dot
            ) || tok.as_ident().is_some();

            if is_path_part {
                end = self.tokens[self.pos].span.end;
                self.advance();
            } else {
                break;
            }
        }

        Ok(self.source[start..end].to_string())
    }

    fn parse_route_config(&mut self) -> Result<RouteConfig, ParseError> {
        let mut config = RouteConfig::default();
        if self.peek() != Some(&Token::LParen) {
            return Ok(config);
        }
        self.advance(); // consume LParen

        while self.peek() != Some(&Token::RParen) && self.peek().is_some() {
            match self.peek() {
                Some(Token::Post) => {
                    self.advance();
                    self.expect(Token::Colon)?;
                    config.method = Some(HttpMethod::Post);
                    config.payload_type = Some(self.parse_type_ref()?);
                }
                Some(Token::Get) => {
                    self.advance();
                    self.expect(Token::Colon)?;
                    config.method = Some(HttpMethod::Get);
                    config.payload_type = Some(self.parse_type_ref()?);
                }
                Some(Token::Put) => {
                    self.advance();
                    self.expect(Token::Colon)?;
                    config.method = Some(HttpMethod::Put);
                    config.payload_type = Some(self.parse_type_ref()?);
                }
                Some(Token::Delete) => {
                    self.advance();
                    self.expect(Token::Colon)?;
                    config.method = Some(HttpMethod::Delete);
                    config.payload_type = Some(self.parse_type_ref()?);
                }
                Some(Token::Auth) => {
                    self.advance();
                    self.expect(Token::Colon)?;
                    match self.peek() {
                        Some(Token::True) => {
                            self.advance();
                            config.auth = Some(true);
                        }
                        Some(Token::False) => {
                            self.advance();
                            config.auth = Some(false);
                        }
                        _ => {
                            return Err(ParseError {
                                message: "Expected boolean literal for auth config".to_string(),
                                span: self.peek_span(),
                            })
                        }
                    }
                }
                Some(Token::RateLimit) => {
                    self.advance();
                    self.expect(Token::Colon)?;
                    if let Some(Token::IntLiteral(limit)) = self.peek() {
                        let limit = *limit;
                        self.advance();
                        config.rate_limit = Some(limit);
                    } else {
                        return Err(ParseError {
                            message: "Expected integer literal for rate_limit".to_string(),
                            span: self.peek_span(),
                        });
                    }
                }
                Some(other) => {
                    return Err(ParseError {
                        message: format!("Unknown route option: {:?}", other),
                        span: self.peek_span(),
                    })
                }
                None => break,
            }

            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        self.expect(Token::RParen)?;
        Ok(config)
    }

    // -------------------------------------------------------------
    // UI Island & Function Declarations
    // -------------------------------------------------------------
    fn parse_ui_decl(&mut self) -> Result<UiDecl, ParseError> {
        self.expect(Token::Ui)?;
        let (name, _) = self.expect_ident()?;
        let params = self.parse_param_list()?;
        let body = self.parse_block()?;
        Ok(UiDecl { name, params, body })
    }

    fn parse_fn_decl(&mut self) -> Result<FnDecl, ParseError> {
        self.expect(Token::Fn)?;
        let (name, _) = self.expect_ident()?;
        let params = self.parse_param_list()?;
        let return_type = if self.peek() == Some(&Token::Arrow) {
            self.advance();
            Some(self.parse_type_ref()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        Ok(FnDecl {
            name,
            params,
            return_type,
            body,
        })
    }

    fn parse_param_list(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect(Token::LParen)?;
        let mut params = Vec::new();

        while self.peek() != Some(&Token::RParen) && self.peek().is_some() {
            let (name, _) = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let ty = self.parse_type_ref()?;

            let default = if self.peek() == Some(&Token::Eq) {
                self.advance();
                Some(self.parse_expr(0)?)
            } else {
                None
            };

            params.push(Param { name, ty, default });

            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        self.expect(Token::RParen)?;
        Ok(params)
    }

    // -------------------------------------------------------------
    // Statements & Blocks
    // -------------------------------------------------------------
    pub fn parse_block(&mut self) -> Result<Block, ParseError> {
        self.expect(Token::LBrace)?;
        let mut stmts = Vec::new();

        while self.peek() != Some(&Token::RBrace) && self.peek().is_some() {
            stmts.push(self.parse_stmt()?);
        }

        self.expect(Token::RBrace)?;
        Ok(Block { stmts })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek() {
            Some(Token::Let) => self.parse_let_stmt(),
            Some(Token::Return) => self.parse_return_stmt(),
            Some(Token::Throw) => self.parse_throw_stmt(),
            Some(Token::Action) => self.parse_action_stmt(),
            Some(Token::If) => self.parse_if_stmt(),
            Some(Token::For) => self.parse_for_stmt(),
            _ => {
                let expr = self.parse_expr(0)?;
                if self.peek() == Some(&Token::Semicolon) {
                    self.advance();
                }
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn parse_let_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.expect(Token::Let)?;
        let (name, _) = self.expect_ident()?;
        let ty = if self.peek() == Some(&Token::Colon) {
            self.advance();
            Some(self.parse_type_ref()?)
        } else {
            None
        };

        self.expect(Token::Eq)?;
        let val = self.parse_expr(0)?;
        if self.peek() == Some(&Token::Semicolon) {
            self.advance();
        }

        Ok(Stmt::Let { name, ty, val })
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.expect(Token::Return)?;
        let val = if self.peek() != Some(&Token::Semicolon)
            && self.peek() != Some(&Token::RBrace)
            && self.peek().is_some()
        {
            Some(self.parse_expr(0)?)
        } else {
            None
        };

        if self.peek() == Some(&Token::Semicolon) {
            self.advance();
        }

        Ok(Stmt::Return(val))
    }

    fn parse_throw_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.expect(Token::Throw)?;
        let code = if let Some(Token::IntLiteral(c)) = self.peek() {
            let c = *c;
            self.advance();
            c
        } else {
            return Err(ParseError {
                message: "Expected integer status code after `throw`".to_string(),
                span: self.peek_span(),
            });
        };

        let message = if self.peek() != Some(&Token::Semicolon)
            && self.peek() != Some(&Token::RBrace)
            && self.peek().is_some()
        {
            Some(self.parse_expr(0)?)
        } else {
            None
        };

        if self.peek() == Some(&Token::Semicolon) {
            self.advance();
        }

        Ok(Stmt::Throw { code, message })
    }

    fn parse_action_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.expect(Token::Action)?;
        let (name, _) = self.expect_ident()?;
        let params = self.parse_param_list()?;
        let return_type = if self.peek() == Some(&Token::Arrow) {
            self.advance();
            Some(self.parse_type_ref()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        Ok(Stmt::Action {
            name,
            params,
            return_type,
            body,
        })
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.expect(Token::If)?;
        let cond = self.parse_expr(0)?;
        let then_branch = self.parse_block()?;

        let else_branch = if self.peek() == Some(&Token::Else) {
            self.advance();
            if self.peek() == Some(&Token::If) {
                Some(ElseBranch::If(Box::new(self.parse_if_stmt()?)))
            } else {
                Some(ElseBranch::Block(self.parse_block()?))
            }
        } else {
            None
        };

        Ok(Stmt::If {
            cond,
            then_branch,
            else_branch,
        })
    }

    fn parse_for_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.expect(Token::For)?;
        let (var, _) = self.expect_ident()?;
        self.expect(Token::In)?;
        let iter = self.parse_expr(0)?;
        let body = self.parse_block()?;
        Ok(Stmt::For { var, iter, body })
    }

    // -------------------------------------------------------------
    // Pratt Expression Parser
    // -------------------------------------------------------------
    pub fn parse_expr(&mut self, min_bp: u8) -> Result<ExprId, ParseError> {
        let mut lhs = self.parse_prefix_expr()?;

        loop {
            // Postfix: member access (.field) or call (...)
            if self.peek() == Some(&Token::Dot) {
                self.advance();
                let (field, field_span) = self.expect_ident()?;
                let start = self.arena.get_span(lhs).start;
                let end = field_span.end;
                lhs = self
                    .arena
                    .alloc_expr(Expr::MemberAccess { object: lhs, field }, start..end);
                continue;
            }

            if self.peek() == Some(&Token::LParen) {
                let (args, rparen_span) = self.parse_call_args()?;
                let start = self.arena.get_span(lhs).start;
                let end = rparen_span.end;
                lhs = self
                    .arena
                    .alloc_expr(Expr::Call { callee: lhs, args }, start..end);
                continue;
            }

            // Check if next token is on a new line and starts a new statement / JSX element
            if self.has_leading_newline() {
                if self.peek() == Some(&Token::Lt)
                    && matches!(self.peek_ahead(1), Some(tok) if tok.as_ident().is_some())
                {
                    break;
                }
                if matches!(
                    self.peek(),
                    Some(
                        Token::Let
                            | Token::Return
                            | Token::Throw
                            | Token::Action
                            | Token::If
                            | Token::For
                            | Token::RBrace
                    )
                ) {
                    break;
                }
            }

            let op = match self.peek() {
                Some(Token::Eq) => BinaryOp::Assign,
                Some(Token::Plus) => BinaryOp::Add,
                Some(Token::Minus) => BinaryOp::Sub,
                Some(Token::Star) => BinaryOp::Mul,
                Some(Token::Slash) => BinaryOp::Div,
                Some(Token::Percent) => BinaryOp::Mod,
                Some(Token::EqEq) => BinaryOp::Eq,
                Some(Token::NotEq) => BinaryOp::NotEq,
                Some(Token::Lt) => BinaryOp::Lt,
                Some(Token::LtEq) => BinaryOp::LtEq,
                Some(Token::Gt) => BinaryOp::Gt,
                Some(Token::GtEq) => BinaryOp::GtEq,
                Some(Token::AndAnd) => BinaryOp::And,
                Some(Token::OrOr) => BinaryOp::Or,
                Some(Token::DoubleQuestion) => BinaryOp::Coalesce,
                Some(Token::PlusEq) => BinaryOp::AddAssign,
                Some(Token::MinusEq) => BinaryOp::SubAssign,
                _ => break,
            };

            let (l_bp, r_bp) = infix_binding_power(op);
            if l_bp < min_bp {
                break;
            }

            self.advance();

            // Special case: `?? throw 401 "..."`
            let rhs = if op == BinaryOp::Coalesce && self.peek() == Some(&Token::Throw) {
                let throw_span = self.expect(Token::Throw)?;
                let (code, code_span) = if let Some(Token::IntLiteral(c)) = self.peek() {
                    let c = *c;
                    let s = self.peek_span();
                    self.advance();
                    (c, s)
                } else {
                    return Err(ParseError {
                        message:
                            "Expected integer status code after `throw` in coalesce expression"
                                .to_string(),
                        span: self.peek_span(),
                    });
                };
                let (msg, end) = if self.peek() != Some(&Token::Semicolon)
                    && self.peek() != Some(&Token::RBrace)
                    && self.peek() != Some(&Token::RParen)
                    && self.peek().is_some()
                {
                    let m = self.parse_expr(r_bp)?;
                    let e = self.arena.get_span(m).end;
                    (Some(m), e)
                } else {
                    (None, code_span.end)
                };
                self.arena
                    .alloc_expr(Expr::Throw { code, message: msg }, throw_span.start..end)
            } else {
                self.parse_expr(r_bp)?
            };

            let start = self.arena.get_span(lhs).start;
            let end = self.arena.get_span(rhs).end;
            lhs = self
                .arena
                .alloc_expr(Expr::Binary { op, lhs, rhs }, start..end);
        }

        Ok(lhs)
    }

    fn parse_prefix_expr(&mut self) -> Result<ExprId, ParseError> {
        match self.peek() {
            Some(Token::Bang) => {
                let op_span = self.expect(Token::Bang)?;
                let expr = self.parse_expr(PREFIX_BP)?;
                let end = self.arena.get_span(expr).end;
                Ok(self.arena.alloc_expr(
                    Expr::Unary {
                        op: UnaryOp::Not,
                        expr,
                    },
                    op_span.start..end,
                ))
            }
            Some(Token::Minus) => {
                let op_span = self.expect(Token::Minus)?;
                let expr = self.parse_expr(PREFIX_BP)?;
                let end = self.arena.get_span(expr).end;
                Ok(self.arena.alloc_expr(
                    Expr::Unary {
                        op: UnaryOp::Neg,
                        expr,
                    },
                    op_span.start..end,
                ))
            }
            _ => self.parse_primary_expr(),
        }
    }

    fn parse_primary_expr(&mut self) -> Result<ExprId, ParseError> {
        let span = self.peek_span();
        match self.peek() {
            Some(Token::IntLiteral(n)) => {
                let n = *n;
                self.advance();
                Ok(self.arena.alloc_expr(Expr::Literal(Literal::Int(n)), span))
            }
            Some(Token::FloatLiteral(f)) => {
                let f = *f;
                self.advance();
                Ok(self
                    .arena
                    .alloc_expr(Expr::Literal(Literal::Float(f)), span))
            }
            Some(Token::StringLiteral(s)) => {
                let s = s.clone();
                self.advance();
                Ok(self
                    .arena
                    .alloc_expr(Expr::Literal(Literal::String(s)), span))
            }
            Some(Token::True) => {
                self.advance();
                Ok(self
                    .arena
                    .alloc_expr(Expr::Literal(Literal::Bool(true)), span))
            }
            Some(Token::False) => {
                self.advance();
                Ok(self
                    .arena
                    .alloc_expr(Expr::Literal(Literal::Bool(false)), span))
            }
            Some(Token::Null) => {
                self.advance();
                Ok(self.arena.alloc_expr(Expr::Literal(Literal::Null), span))
            }
            Some(Token::LParen) => {
                self.advance();
                let inner = self.parse_expr(0)?;
                self.expect(Token::RParen)?;
                Ok(inner)
            }
            Some(Token::LBrace) => self.parse_object_literal(),
            Some(Token::LBracket) => self.parse_array_literal(),
            Some(Token::Lt) if matches!(self.peek_ahead(1), Some(tok) if tok.as_ident().is_some()) =>
            {
                let jsx = self.parse_jsx_element()?;
                let jsx_span = jsx.span.clone();
                Ok(self.arena.alloc_expr(Expr::Jsx(Box::new(jsx)), jsx_span))
            }
            Some(tok) if tok.as_ident().is_some() => {
                let name = tok.as_ident().unwrap();
                self.advance();
                Ok(self.arena.alloc_expr(Expr::Ident(name), span))
            }
            Some(other) => Err(ParseError {
                message: format!("Expected expression, found {:?}", other),
                span,
            }),
            None => Err(ParseError {
                message: "Expected expression, found end of file".to_string(),
                span,
            }),
        }
    }

    fn parse_object_literal(&mut self) -> Result<ExprId, ParseError> {
        let lbrace_span = self.expect(Token::LBrace)?;
        let mut fields = Vec::new();

        while self.peek() != Some(&Token::RBrace) && self.peek().is_some() {
            let (name, _) = self.expect_ident()?;
            let val = if self.peek() == Some(&Token::Colon) {
                self.advance();
                Some(self.parse_expr(0)?)
            } else {
                None // Shorthand `{ email }`
            };

            fields.push(ObjectField { name, val });

            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        let rbrace_span = self.expect(Token::RBrace)?;
        let span = lbrace_span.start..rbrace_span.end;
        Ok(self.arena.alloc_expr(Expr::ObjectLit(fields), span))
    }

    fn parse_array_literal(&mut self) -> Result<ExprId, ParseError> {
        let lbracket_span = self.expect(Token::LBracket)?;
        let mut items = Vec::new();

        while self.peek() != Some(&Token::RBracket) && self.peek().is_some() {
            items.push(self.parse_expr(0)?);
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        let rbracket_span = self.expect(Token::RBracket)?;
        let span = lbracket_span.start..rbracket_span.end;
        Ok(self.arena.alloc_expr(Expr::ArrayLit(items), span))
    }

    fn parse_call_args(&mut self) -> Result<(Vec<CallArg>, Range<usize>), ParseError> {
        self.expect(Token::LParen)?;
        let mut args = Vec::new();

        while self.peek() != Some(&Token::RParen) && self.peek().is_some() {
            if let (Some(tok), Some(Token::Colon)) = (self.peek(), self.peek_ahead(1)) {
                if let Some(name) = tok.as_ident() {
                    self.advance(); // consume ident
                    self.advance(); // consume colon
                    let val = self.parse_expr(0)?;
                    args.push(CallArg::Named { name, val });
                    if self.peek() == Some(&Token::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                    continue;
                }
            }

            let val = self.parse_expr(0)?;
            args.push(CallArg::Positional(val));

            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        let rparen_span = self.expect(Token::RParen)?;
        Ok((args, rparen_span))
    }

    // -------------------------------------------------------------
    // JSX Element Parser
    // -------------------------------------------------------------
    fn parse_jsx_element(&mut self) -> Result<JsxElement, ParseError> {
        let start_span = self.expect(Token::Lt)?;
        let (tag, _) = self.expect_ident()?;
        let mut attrs = Vec::new();

        // Parse attributes until `>` or `/>`
        while self.peek() != Some(&Token::Gt)
            && self.peek() != Some(&Token::SlashGt)
            && self.peek().is_some()
        {
            let attr_name = match self.peek() {
                Some(Token::At) => {
                    self.advance();
                    let (n, _) = self.expect_ident()?;
                    format!("@{}", n)
                }
                Some(tok) if tok.as_ident().is_some() => {
                    let n = tok.as_ident().unwrap();
                    self.advance();
                    n
                }
                Some(other) => {
                    return Err(ParseError {
                        message: format!("Expected attribute name, found {:?}", other),
                        span: self.peek_span(),
                    })
                }
                None => break,
            };

            let is_event = attr_name.starts_with('@') || attr_name.starts_with("on");
            let attr_val = if self.peek() == Some(&Token::Eq) {
                self.advance();
                match self.peek() {
                    Some(Token::StringLiteral(s)) => {
                        let s = s.clone();
                        self.advance();
                        Some(JsxAttrValue::String(s))
                    }
                    Some(Token::LBrace) => {
                        if is_event {
                            let block = self.parse_block()?;
                            Some(JsxAttrValue::Block(block))
                        } else {
                            self.advance(); // consume LBrace
                            let expr = self.parse_expr(0)?;
                            self.expect(Token::RBrace)?;
                            Some(JsxAttrValue::Expr(expr))
                        }
                    }
                    _ => None,
                }
            } else {
                None
            };

            attrs.push(JsxAttr {
                name: attr_name,
                value: attr_val,
            });
        }

        // Self-closing tag `<Tag ... />`
        if self.peek() == Some(&Token::SlashGt) {
            let slash_gt_span = self.peek_span();
            self.advance();
            return Ok(JsxElement {
                tag,
                attrs,
                children: Vec::new(),
                span: start_span.start..slash_gt_span.end,
            });
        }

        self.expect(Token::Gt)?;
        let mut children = Vec::new();

        // Parse children until closing tag `</tag>`
        while self.peek().is_some() {
            if self.peek() == Some(&Token::LtSlash) {
                break;
            }

            match self.peek() {
                Some(Token::Lt) if matches!(self.peek_ahead(1), Some(tok) if tok.as_ident().is_some()) =>
                {
                    children.push(JsxChild::Element(self.parse_jsx_element()?));
                }
                Some(Token::LBrace) => {
                    self.advance();
                    let expr = self.parse_expr(0)?;
                    self.expect(Token::RBrace)?;
                    children.push(JsxChild::Expr(expr));
                }
                Some(Token::StringLiteral(s)) => {
                    let s = s.clone();
                    self.advance();
                    children.push(JsxChild::Text(s));
                }
                Some(tok)
                    if tok != &Token::Lt && tok != &Token::LBrace && tok != &Token::LtSlash =>
                {
                    let mut text = String::new();
                    while let Some(t) = self.peek() {
                        if t == &Token::Lt || t == &Token::LtSlash || t == &Token::LBrace {
                            break;
                        }
                        let span = self.peek_span();
                        text.push_str(&self.source[span]);
                        self.advance();
                    }
                    if !text.trim().is_empty() {
                        children.push(JsxChild::Text(text));
                    }
                }
                Some(other) => {
                    return Err(ParseError {
                        message: format!("Unexpected token in JSX child: {:?}", other),
                        span: self.peek_span(),
                    })
                }
                None => break,
            }
        }

        self.expect(Token::LtSlash)?;
        let (closing_tag, closing_span) = self.expect_ident()?;
        if closing_tag != tag {
            return Err(ParseError {
                message: format!(
                    "Mismatched JSX closing tag: expected `</{}>`, found `</{}>`",
                    tag, closing_tag
                ),
                span: closing_span,
            });
        }
        let gt_span = self.expect(Token::Gt)?;

        Ok(JsxElement {
            tag,
            attrs,
            children,
            span: start_span.start..gt_span.end,
        })
    }
}

fn infix_binding_power(op: BinaryOp) -> (u8, u8) {
    match op {
        BinaryOp::Assign | BinaryOp::AddAssign | BinaryOp::SubAssign => (2, 1), // Right-associative
        BinaryOp::Coalesce => (3, 4),
        BinaryOp::Or => (5, 6),
        BinaryOp::And => (7, 8),
        BinaryOp::Eq | BinaryOp::NotEq => (9, 10),
        BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => (11, 12),
        BinaryOp::Add | BinaryOp::Sub => (13, 14),
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => (15, 16),
    }
}
