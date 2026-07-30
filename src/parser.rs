//! Recursive-descent parser with Pratt precedence for expressions.

use crate::ast::*;
use crate::diagnostics::Diagnostics;
use crate::span::{SourceFile, Span};
use crate::token::{Token, TokenKind};

pub fn parse(
    _file: &SourceFile,
    tokens: &[Token],
    diagnostics: &mut Diagnostics,
) -> Option<Program> {
    let mut parser = Parser {
        tokens,
        pos: 0,
        diagnostics,
    };
    let program = parser.parse_program();
    if parser.diagnostics.has_errors() {
        None
    } else {
        Some(program)
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    diagnostics: &'a mut Diagnostics,
}

impl<'a> Parser<'a> {
    fn parse_program(&mut self) -> Program {
        let mut items = Vec::new();
        self.skip_newlines();
        while !self.is_eof() {
            if let Some(item) = self.parse_item() {
                items.push(item);
            } else {
                // Recover: skip to next newline / top-level keyword.
                self.synchronize();
            }
            self.skip_newlines();
        }
        Program { items }
    }

    fn parse_item(&mut self) -> Option<Item> {
        match &self.current().kind {
            TokenKind::Fn => self.parse_function().map(Item::Function),
            TokenKind::Struct => self.parse_struct().map(Item::Struct),
            TokenKind::Enum => self.parse_enum().map(Item::Enum),
            TokenKind::Extern => self.parse_extern().map(Item::Extern),
            _ => {
                let tok = self.current().clone();
                self.diagnostics.error_at(
                    tok.span,
                    format!(
                        "expected item (`fn`, `struct`, `enum`, or `extern`), found {}",
                        tok.kind
                    ),
                );
                None
            }
        }
    }

    fn parse_enum(&mut self) -> Option<EnumDef> {
        let start = self.bump().span;
        let (name, _) = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        self.expect_newline_or_skip();
        self.expect(&TokenKind::Indent)?;
        let mut variants = Vec::new();
        while !matches!(self.current().kind, TokenKind::Dedent | TokenKind::Eof) {
            self.skip_newlines();
            if matches!(self.current().kind, TokenKind::Dedent | TokenKind::Eof) {
                break;
            }
            let (vname, vspan) = self.expect_ident()?;
            let mut fields = Vec::new();
            if self.eat(&TokenKind::LParen) {
                if !matches!(self.current().kind, TokenKind::RParen) {
                    loop {
                        let (fname, fspan) = self.expect_ident()?;
                        self.expect(&TokenKind::Colon)?;
                        let ty = self.parse_type()?;
                        fields.push(Field {
                            name: fname,
                            ty,
                            span: fspan,
                        });
                        if self.eat(&TokenKind::Comma) {
                            continue;
                        }
                        break;
                    }
                }
                self.expect(&TokenKind::RParen)?;
            }
            variants.push(VariantDef {
                name: vname,
                fields,
                span: vspan.merge(self.previous_span()),
            });
            self.skip_newlines();
        }
        self.expect(&TokenKind::Dedent)?;
        if variants.is_empty() {
            self.diagnostics.error_at(
                start,
                format!("enum `{name}` must have at least one variant"),
            );
        }
        Some(EnumDef {
            name,
            variants,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_struct(&mut self) -> Option<StructDef> {
        let start = self.bump().span;
        let mut is_c_repr = false;
        // Optional: struct @c Name  or  struct Name  — use attribute-like: `struct c Name`
        // Spec: `struct Name:` or `struct #repr(C) Name:` — keep simple: `struct Name:`
        // For C-repr: `struct Name c:` field  — better: keyword after name: we use
        // `#[repr(C)]` style is heavy. Use: `struct Name:` and `cstruct Name:` via Ident check.
        // Plan: `struct Name:` normal, and `extern struct Name:` for C layout.
        // Simpler Phase 4 approach already in ast: is_c_repr flag.
        // Syntax: `struct Name:` or `struct c Name:` where `c` marks C layout.
        let name_tok = self.expect_ident()?;
        let mut name = name_tok.0;
        if name == "c" {
            is_c_repr = true;
            let real = self.expect_ident()?;
            name = real.0;
        }
        self.expect(&TokenKind::Colon)?;
        self.expect_newline_or_skip();
        self.expect(&TokenKind::Indent)?;
        let mut fields = Vec::new();
        while !matches!(self.current().kind, TokenKind::Dedent | TokenKind::Eof) {
            self.skip_newlines();
            if matches!(self.current().kind, TokenKind::Dedent | TokenKind::Eof) {
                break;
            }
            let (fname, fspan) = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let ty = self.parse_type()?;
            fields.push(Field {
                name: fname,
                ty,
                span: fspan,
            });
            self.skip_newlines();
        }
        self.expect(&TokenKind::Dedent)?;
        Some(StructDef {
            name,
            fields,
            is_c_repr,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_extern(&mut self) -> Option<ExternFn> {
        let start = self.bump().span;
        // extern "C" fn name(...) -> Type
        match &self.current().kind {
            TokenKind::StringLit(s) if s == "C" => {
                self.bump();
            }
            _ => {
                self.diagnostics
                    .error_at(self.current().span, "expected `\"C\"` after `extern`");
            }
        }
        self.expect(&TokenKind::Fn)?;
        let (name, _) = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen)?;
        let return_type = if self.eat(&TokenKind::Arrow) {
            self.parse_type()?
        } else {
            TypeExpr::unit(self.current().span)
        };
        Some(ExternFn {
            name,
            params,
            return_type,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_function(&mut self) -> Option<Function> {
        let start = self.bump().span;
        let (name, _) = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen)?;
        let return_type = if self.eat(&TokenKind::Arrow) {
            self.parse_type()?
        } else {
            TypeExpr::unit(self.current().span)
        };
        self.expect(&TokenKind::Colon)?;
        let body = self.parse_block()?;
        let span = start.merge(body.span);
        Some(Function {
            name,
            params,
            return_type,
            body,
            span,
        })
    }

    fn parse_params(&mut self) -> Option<Vec<Param>> {
        let mut params = Vec::new();
        if matches!(self.current().kind, TokenKind::RParen) {
            return Some(params);
        }
        loop {
            let (name, nspan) = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let ty = self.parse_type()?;
            params.push(Param {
                name,
                ty,
                span: nspan,
            });
            if self.eat(&TokenKind::Comma) {
                continue;
            }
            break;
        }
        Some(params)
    }

    fn parse_block(&mut self) -> Option<Block> {
        self.skip_newlines();
        let start = self.current().span;
        self.expect(&TokenKind::Indent)?;
        let mut stmts = Vec::new();
        while !matches!(self.current().kind, TokenKind::Dedent | TokenKind::Eof) {
            self.skip_newlines();
            if matches!(self.current().kind, TokenKind::Dedent | TokenKind::Eof) {
                break;
            }
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
            } else {
                self.synchronize_stmt();
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::Dedent)?;
        Some(Block {
            stmts,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        match &self.current().kind {
            TokenKind::Let => self.parse_let(),
            TokenKind::Return => self.parse_return(),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Arena => self.parse_arena(),
            TokenKind::Unsafe => self.parse_unsafe(),
            TokenKind::Match => self.parse_match(),
            _ => {
                let expr = self.parse_expr()?;
                if self.eat(&TokenKind::Eq) {
                    let value = self.parse_expr()?;
                    let span = expr.span.merge(value.span);
                    Some(Stmt::Assign {
                        target: expr,
                        value,
                        span,
                    })
                } else {
                    let span = expr.span;
                    Some(Stmt::Expr { expr, span })
                }
            }
        }
    }

    fn parse_let(&mut self) -> Option<Stmt> {
        let start = self.bump().span;
        let mutable = self.eat(&TokenKind::Mut);
        let (name, _) = self.expect_ident()?;
        let ty = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_expr()?;
        Some(Stmt::Let {
            name,
            mutable,
            ty,
            span: start.merge(value.span),
            value,
        })
    }

    fn parse_return(&mut self) -> Option<Stmt> {
        let start = self.bump().span;
        if matches!(
            self.current().kind,
            TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof
        ) {
            return Some(Stmt::Return {
                value: None,
                span: start,
            });
        }
        let value = self.parse_expr()?;
        Some(Stmt::Return {
            span: start.merge(value.span),
            value: Some(value),
        })
    }

    fn parse_if(&mut self) -> Option<Stmt> {
        let start = self.bump().span;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::Colon)?;
        let then_block = self.parse_block()?;
        self.skip_newlines();
        let else_block = if self.eat(&TokenKind::Else) {
            if self.eat(&TokenKind::Colon) {
                Some(self.parse_block()?)
            } else if matches!(self.current().kind, TokenKind::If) {
                // else if → wrap as block with single if stmt
                let nested = self.parse_if()?;
                Some(Block {
                    span: nested.span_of(),
                    stmts: vec![nested],
                })
            } else {
                self.diagnostics
                    .error_at(self.current().span, "expected `:` or `if` after `else`");
                None
            }
        } else {
            None
        };
        Some(Stmt::If {
            cond,
            then_block,
            else_block,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_while(&mut self) -> Option<Stmt> {
        let start = self.bump().span;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::Colon)?;
        let body = self.parse_block()?;
        Some(Stmt::While {
            cond,
            span: start.merge(body.span),
            body,
        })
    }

    fn parse_arena(&mut self) -> Option<Stmt> {
        let start = self.bump().span;
        self.expect(&TokenKind::Colon)?;
        let body = self.parse_block()?;
        Some(Stmt::Arena {
            span: start.merge(body.span),
            body,
        })
    }

    fn parse_unsafe(&mut self) -> Option<Stmt> {
        let start = self.bump().span;
        self.expect(&TokenKind::Colon)?;
        let body = self.parse_block()?;
        Some(Stmt::Unsafe {
            span: start.merge(body.span),
            body,
        })
    }

    fn parse_match(&mut self) -> Option<Stmt> {
        let start = self.bump().span;
        let scrutinee = self.parse_expr()?;
        self.expect(&TokenKind::Colon)?;
        self.expect_newline_or_skip();
        self.expect(&TokenKind::Indent)?;
        let mut arms = Vec::new();
        while !matches!(self.current().kind, TokenKind::Dedent | TokenKind::Eof) {
            self.skip_newlines();
            if matches!(self.current().kind, TokenKind::Dedent | TokenKind::Eof) {
                break;
            }
            self.expect(&TokenKind::Case)?;
            let pattern = self.parse_pattern()?;
            self.expect(&TokenKind::Colon)?;
            let body = self.parse_block()?;
            let span = pattern.span.merge(body.span);
            arms.push(MatchArm {
                pattern,
                body,
                span,
            });
            self.skip_newlines();
        }
        self.expect(&TokenKind::Dedent)?;
        if arms.is_empty() {
            self.diagnostics
                .error_at(start, "`match` requires at least one `case` arm");
        }
        Some(Stmt::Match {
            scrutinee,
            arms,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_pattern(&mut self) -> Option<Pattern> {
        let tok = self.current().clone();
        match &tok.kind {
            TokenKind::Ident(name) if name == "_" => {
                self.bump();
                Some(Pattern {
                    kind: PatternKind::Wildcard,
                    span: tok.span,
                })
            }
            TokenKind::True => {
                self.bump();
                Some(Pattern {
                    kind: PatternKind::Bool(true),
                    span: tok.span,
                })
            }
            TokenKind::False => {
                self.bump();
                Some(Pattern {
                    kind: PatternKind::Bool(false),
                    span: tok.span,
                })
            }
            TokenKind::Ident(name) => {
                let enum_name = name.clone();
                self.bump();
                if self.eat(&TokenKind::ColonColon) {
                    let (variant, _) = self.expect_ident()?;
                    let mut fields = Vec::new();
                    if self.eat(&TokenKind::LParen) {
                        if !matches!(self.current().kind, TokenKind::RParen) {
                            loop {
                                fields.push(self.parse_pattern()?);
                                if self.eat(&TokenKind::Comma) {
                                    continue;
                                }
                                break;
                            }
                        }
                        self.expect(&TokenKind::RParen)?;
                    }
                    Some(Pattern {
                        kind: PatternKind::Variant {
                            enum_name,
                            variant,
                            fields,
                        },
                        span: tok.span.merge(self.previous_span()),
                    })
                } else {
                    Some(Pattern {
                        kind: PatternKind::Binding(enum_name),
                        span: tok.span,
                    })
                }
            }
            _ => {
                self.diagnostics
                    .error_at(tok.span, format!("expected pattern, found {}", tok.kind));
                None
            }
        }
    }

    fn parse_type(&mut self) -> Option<TypeExpr> {
        let start = self.current().span;
        if self.eat(&TokenKind::Amp) {
            let mutable = self.eat(&TokenKind::Mut);
            let inner = self.parse_type()?;
            return Some(TypeExpr {
                kind: TypeExprKind::Ref {
                    mutable,
                    inner: Box::new(inner),
                },
                span: start.merge(self.previous_span()),
            });
        }
        if self.eat(&TokenKind::LParen) {
            self.expect(&TokenKind::RParen)?;
            return Some(TypeExpr::unit(start.merge(self.previous_span())));
        }
        if self.eat(&TokenKind::LBracket) {
            let elem = self.parse_type()?;
            if self.eat(&TokenKind::Semi) {
                let len = match &self.current().kind {
                    TokenKind::Int(n) => {
                        let n = *n;
                        self.bump();
                        if n < 0 {
                            self.diagnostics.error_at(
                                self.previous_span(),
                                "array length must be a non-negative integer",
                            );
                        }
                        n.max(0)
                    }
                    _ => {
                        let tok = self.current().clone();
                        self.diagnostics.error_at(
                            tok.span,
                            format!("expected array length integer, found {}", tok.kind),
                        );
                        return None;
                    }
                };
                self.expect(&TokenKind::RBracket)?;
                return Some(TypeExpr {
                    kind: TypeExprKind::Array {
                        elem: Box::new(elem),
                        len,
                    },
                    span: start.merge(self.previous_span()),
                });
            }
            self.expect(&TokenKind::RBracket)?;
            return Some(TypeExpr {
                kind: TypeExprKind::Slice {
                    elem: Box::new(elem),
                },
                span: start.merge(self.previous_span()),
            });
        }
        let (name, span) = self.expect_ident()?;
        if name == "unit" {
            Some(TypeExpr::unit(span))
        } else {
            Some(TypeExpr::named(name, span))
        }
    }

    fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_precedence(0)
    }

    fn parse_precedence(&mut self, min_prec: u8) -> Option<Expr> {
        let mut left = self.parse_unary()?;
        while let Some((op, prec, lassoc)) = binary_info(&self.current().kind) {
            if prec < min_prec {
                break;
            }
            self.bump();
            let next_min = if lassoc { prec + 1 } else { prec };
            let right = self.parse_precedence(next_min)?;
            let span = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }
        Some(left)
    }

    fn parse_unary(&mut self) -> Option<Expr> {
        let start = self.current().span;
        if self.eat(&TokenKind::Minus) {
            let expr = self.parse_unary()?;
            return Some(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                },
                span: start.merge(self.previous_span()),
            });
        }
        if self.eat(&TokenKind::Not) {
            let expr = self.parse_unary()?;
            return Some(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                },
                span: start.merge(self.previous_span()),
            });
        }
        if self.eat(&TokenKind::Amp) {
            let mutable = self.eat(&TokenKind::Mut);
            let expr = self.parse_unary()?;
            return Some(Expr {
                kind: ExprKind::Ref {
                    mutable,
                    expr: Box::new(expr),
                },
                span: start.merge(self.previous_span()),
            });
        }
        if self.eat(&TokenKind::Star) {
            let expr = self.parse_unary()?;
            return Some(Expr {
                kind: ExprKind::Deref {
                    expr: Box::new(expr),
                },
                span: start.merge(self.previous_span()),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Option<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.eat(&TokenKind::LParen) {
                let mut args = Vec::new();
                if !matches!(self.current().kind, TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if self.eat(&TokenKind::Comma) {
                            continue;
                        }
                        break;
                    }
                }
                self.expect(&TokenKind::RParen)?;
                let span = expr.span.merge(self.previous_span());
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                };
            } else if self.eat(&TokenKind::Dot) {
                let (field, _) = self.expect_ident()?;
                let span = expr.span.merge(self.previous_span());
                expr = Expr {
                    kind: ExprKind::Field {
                        base: Box::new(expr),
                        field,
                    },
                    span,
                };
            } else if self.eat(&TokenKind::LBracket) {
                // Index `a[i]` or slice `a[start..end]` / `a[..]` / `a[start..]` / `a[..end]`
                if matches!(self.current().kind, TokenKind::DotDot) {
                    self.bump(); // ..
                    let end = if matches!(self.current().kind, TokenKind::RBracket) {
                        None
                    } else {
                        Some(Box::new(self.parse_expr()?))
                    };
                    self.expect(&TokenKind::RBracket)?;
                    let span = expr.span.merge(self.previous_span());
                    expr = Expr {
                        kind: ExprKind::Slice {
                            base: Box::new(expr),
                            start: None,
                            end,
                        },
                        span,
                    };
                } else {
                    let first = self.parse_expr()?;
                    if self.eat(&TokenKind::DotDot) {
                        let end = if matches!(self.current().kind, TokenKind::RBracket) {
                            None
                        } else {
                            Some(Box::new(self.parse_expr()?))
                        };
                        self.expect(&TokenKind::RBracket)?;
                        let span = expr.span.merge(self.previous_span());
                        expr = Expr {
                            kind: ExprKind::Slice {
                                base: Box::new(expr),
                                start: Some(Box::new(first)),
                                end,
                            },
                            span,
                        };
                    } else {
                        self.expect(&TokenKind::RBracket)?;
                        let span = expr.span.merge(self.previous_span());
                        expr = Expr {
                            kind: ExprKind::Index {
                                base: Box::new(expr),
                                index: Box::new(first),
                            },
                            span,
                        };
                    }
                }
            } else if self.eat(&TokenKind::As) {
                let ty = self.parse_type()?;
                let span = expr.span.merge(ty.span);
                expr = Expr {
                    kind: ExprKind::Cast {
                        expr: Box::new(expr),
                        ty,
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Some(expr)
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        let tok = self.current().clone();
        match tok.kind {
            TokenKind::Int(v) => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Int(v),
                    span: tok.span,
                })
            }
            TokenKind::Float(v) => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Float(v),
                    span: tok.span,
                })
            }
            TokenKind::True => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Bool(true),
                    span: tok.span,
                })
            }
            TokenKind::False => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Bool(false),
                    span: tok.span,
                })
            }
            TokenKind::StringLit(s) => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Str(s),
                    span: tok.span,
                })
            }
            TokenKind::Ident(name) => {
                self.bump();
                // Enum constructor: Name::Variant(...) or Name::Variant
                if self.eat(&TokenKind::ColonColon) {
                    let (variant, _) = self.expect_ident()?;
                    let mut args = Vec::new();
                    if self.eat(&TokenKind::LParen) {
                        if !matches!(self.current().kind, TokenKind::RParen) {
                            loop {
                                args.push(self.parse_expr()?);
                                if self.eat(&TokenKind::Comma) {
                                    continue;
                                }
                                break;
                            }
                        }
                        self.expect(&TokenKind::RParen)?;
                    }
                    return Some(Expr {
                        kind: ExprKind::EnumConstruct {
                            enum_name: name,
                            variant,
                            args,
                        },
                        span: tok.span.merge(self.previous_span()),
                    });
                }
                // Struct literal: Name { field: expr, ... }
                if matches!(self.current().kind, TokenKind::LBrace) {
                    self.bump();
                    let mut fields = Vec::new();
                    self.skip_newlines();
                    while !matches!(self.current().kind, TokenKind::RBrace | TokenKind::Eof) {
                        let (fname, _) = self.expect_ident()?;
                        self.expect(&TokenKind::Colon)?;
                        let value = self.parse_expr()?;
                        fields.push((fname, value));
                        self.eat(&TokenKind::Comma);
                        self.skip_newlines();
                    }
                    self.expect(&TokenKind::RBrace)?;
                    return Some(Expr {
                        kind: ExprKind::StructLit { name, fields },
                        span: tok.span.merge(self.previous_span()),
                    });
                }
                Some(Expr {
                    kind: ExprKind::Name(name),
                    span: tok.span,
                })
            }
            TokenKind::LParen => {
                self.bump();
                if self.eat(&TokenKind::RParen) {
                    // unit literal represented as empty call? use Name("()") — better Bool-like unit
                    // Represent unit as Int(0) typed later — use a sentinel Name
                    return Some(Expr {
                        kind: ExprKind::Name("()".into()),
                        span: tok.span.merge(self.previous_span()),
                    });
                }
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Some(expr)
            }
            TokenKind::LBracket => {
                self.bump();
                let mut elems = Vec::new();
                if !matches!(self.current().kind, TokenKind::RBracket) {
                    loop {
                        elems.push(self.parse_expr()?);
                        if self.eat(&TokenKind::Comma) {
                            if matches!(self.current().kind, TokenKind::RBracket) {
                                break;
                            }
                            continue;
                        }
                        break;
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                Some(Expr {
                    kind: ExprKind::ArrayLit { elems },
                    span: tok.span.merge(self.previous_span()),
                })
            }
            _ => {
                self.diagnostics
                    .error_at(tok.span, format!("expected expression, found {}", tok.kind));
                None
            }
        }
    }

    fn synchronize(&mut self) {
        self.bump();
        while !self.is_eof() {
            if matches!(
                self.current().kind,
                TokenKind::Fn
                    | TokenKind::Struct
                    | TokenKind::Enum
                    | TokenKind::Extern
                    | TokenKind::Eof
            ) {
                break;
            }
            if matches!(self.current().kind, TokenKind::Newline) {
                self.bump();
                break;
            }
            self.bump();
        }
    }

    fn synchronize_stmt(&mut self) {
        while !self.is_eof() {
            if matches!(
                self.current().kind,
                TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof
            ) {
                break;
            }
            self.bump();
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.current().kind, TokenKind::Newline) {
            self.bump();
        }
    }

    fn expect_newline_or_skip(&mut self) {
        if matches!(self.current().kind, TokenKind::Newline) {
            self.skip_newlines();
        }
    }

    fn expect_ident(&mut self) -> Option<(String, Span)> {
        match &self.current().kind {
            TokenKind::Ident(name) => {
                let span = self.current().span;
                let name = name.clone();
                self.bump();
                Some((name, span))
            }
            _ => {
                let tok = self.current().clone();
                self.diagnostics
                    .error_at(tok.span, format!("expected identifier, found {}", tok.kind));
                None
            }
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> Option<Token> {
        if std::mem::discriminant(&self.current().kind) == std::mem::discriminant(kind)
            || self.current().kind == *kind
        {
            // For unit-like tokens without payload, PartialEq works.
            if self.current().kind == *kind
                || matches!(
                    (kind, &self.current().kind),
                    (TokenKind::Ident(_), TokenKind::Ident(_))
                )
            {
                return Some(self.bump());
            }
        }
        // Compare by variant for simple tokens:
        if token_matches(&self.current().kind, kind) {
            return Some(self.bump());
        }
        let tok = self.current().clone();
        self.diagnostics
            .error_at(tok.span, format!("expected {kind}, found {}", tok.kind));
        None
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if token_matches(&self.current().kind, kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn previous_span(&self) -> Span {
        if self.pos == 0 {
            Span::default()
        } else {
            self.tokens[self.pos - 1].span
        }
    }

    fn bump(&mut self) -> Token {
        let tok = self.current().clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn is_eof(&self) -> bool {
        matches!(self.current().kind, TokenKind::Eof)
    }
}

fn token_matches(got: &TokenKind, expected: &TokenKind) -> bool {
    use TokenKind::*;
    match (got, expected) {
        (Ident(_), Ident(_)) => true,
        (Int(_), Int(_)) => true,
        (Float(_), Float(_)) => true,
        (StringLit(_), StringLit(_)) => true,
        _ => got == expected,
    }
}

fn binary_info(kind: &TokenKind) -> Option<(BinaryOp, u8, bool)> {
    // prec, left-assoc
    Some(match kind {
        TokenKind::Or => (BinaryOp::Or, 1, true),
        TokenKind::And => (BinaryOp::And, 2, true),
        TokenKind::EqEq => (BinaryOp::Eq, 3, true),
        TokenKind::NotEq => (BinaryOp::NotEq, 3, true),
        TokenKind::Lt => (BinaryOp::Lt, 4, true),
        TokenKind::LtEq => (BinaryOp::LtEq, 4, true),
        TokenKind::Gt => (BinaryOp::Gt, 4, true),
        TokenKind::GtEq => (BinaryOp::GtEq, 4, true),
        TokenKind::Plus => (BinaryOp::Add, 5, true),
        TokenKind::Minus => (BinaryOp::Sub, 5, true),
        TokenKind::Star => (BinaryOp::Mul, 6, true),
        TokenKind::Slash => (BinaryOp::Div, 6, true),
        TokenKind::Percent => (BinaryOp::Rem, 6, true),
        _ => return None,
    })
}

trait StmtSpan {
    fn span_of(&self) -> Span;
}

impl StmtSpan for Stmt {
    fn span_of(&self) -> Span {
        match self {
            Stmt::Let { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::Expr { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::Arena { span, .. }
            | Stmt::Unsafe { span, .. }
            | Stmt::Match { span, .. } => *span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::span::SourceFile;

    fn parse_ok(src: &str) -> Program {
        let file = SourceFile::new("t.kroa".into(), src.into());
        let mut d = Diagnostics::new();
        let tokens = lexer::lex(&file, &mut d).expect("lex");
        parse(&file, &tokens, &mut d).unwrap_or_else(|| panic!("{}", d.render_all(&file)))
    }

    #[test]
    fn parses_main() {
        let prog = parse_ok("fn main() -> i64:\n    return 1\n");
        assert_eq!(prog.items.len(), 1);
    }

    #[test]
    fn parses_array_literal_index_and_slice() {
        let prog = parse_ok(
            "fn main() -> i64:\n    let a: [i64; 3] = [1, 2, 3]\n    let x = a[1]\n    let s = &a[0..2]\n    return x\n",
        );
        assert_eq!(prog.items.len(), 1);
        let Item::Function(f) = &prog.items[0] else {
            panic!("expected function");
        };
        assert!(matches!(
            f.body.stmts[0],
            Stmt::Let {
                value: Expr {
                    kind: ExprKind::ArrayLit { .. },
                    ..
                },
                ..
            }
        ));
        assert!(matches!(
            f.body.stmts[1],
            Stmt::Let {
                value: Expr {
                    kind: ExprKind::Index { .. },
                    ..
                },
                ..
            }
        ));
        match &f.body.stmts[2] {
            Stmt::Let {
                value:
                    Expr {
                        kind: ExprKind::Ref { expr, .. },
                        ..
                    },
                ..
            } => assert!(matches!(expr.kind, ExprKind::Slice { .. })),
            other => panic!("expected ref-to-slice let, got {other:?}"),
        }
    }
}
