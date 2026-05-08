use crate::parser::ast::{Diagnostic, DiagnosticCode, OutputMode, Severity, Span, Stmt, StmtKind, TokenKind};
use crate::parser::expr::ExprParser;
use crate::parser::lexer::Token;

impl<'s, 'p> ExprParser<'s, 'p> {
    pub(crate) fn parse_program(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        loop {
            match self.lexer.peek_kind() {
                TokenKind::Eof => break,
                // Lexer already emitted a diagnostic; consume and continue
                // so a single bad byte doesn't terminate parsing.
                TokenKind::Invalid => { self.lexer.next(); continue; }
                TokenKind::KwComment => {
                    self.lexer.next();
                    self.skip_to_terminator();
                    continue;
                }
                _ => {}
            }
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
            }
        }
        stmts
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        let start_span = self.lexer.peek().1;
        // Detect assignment: IDENT ':='
        let is_assign = self.lexer.peek_kind() == TokenKind::Ident
            && self.lexer.peek_at(1).0.kind() == TokenKind::Assign;

        if is_assign {
            return self.parse_assign_stmt(start_span);
        }
        self.parse_expr_stmt(start_span)
    }

    fn parse_assign_stmt(&mut self, start_span: Span) -> Option<Stmt> {
        let (ident_tok, _ident_span) = self.lexer.next(); // IDENT
        let name = if let Token::Ident(s) = ident_tok {
            // `intern_str_pub` lowercases internally — pass the raw slice
            // directly to avoid a redundant `to_lowercase()` allocation.
            let raw = &self.src[s.start as usize..s.end as usize];
            self.pool.intern_str_pub(raw)
        } else { unreachable!("expected Ident — guarded by is_assign check") };
        self.lexer.next(); // ':='
        let expr = match self.parse_expr(0) {
            Ok(e) => e,
            Err(()) => { self.synchronise(); return None; }
        };
        let (output, end_span) = self.parse_terminator()?;
        Some(Stmt {
            kind: StmtKind::Assign { lhs: name },
            expr,
            output,
            span: start_span.merge(end_span),
        })
    }

    fn parse_expr_stmt(&mut self, start_span: Span) -> Option<Stmt> {
        let expr = match self.parse_expr(0) {
            Ok(e) => e,
            Err(()) => { self.synchronise(); return None; }
        };
        let (output, end_span) = self.parse_terminator()?;
        Some(Stmt {
            kind: StmtKind::Expr,
            expr,
            output,
            span: start_span.merge(end_span),
        })
    }

    fn parse_terminator(&mut self) -> Option<(OutputMode, Span)> {
        match self.lexer.peek_kind() {
            TokenKind::Semi => {
                let (_, span) = self.lexer.next();
                Some((OutputMode::Display, span))
            }
            TokenKind::Dollar => {
                let (_, span) = self.lexer.next();
                Some((OutputMode::Suppress, span))
            }
            TokenKind::Eof => Some((OutputMode::Display, Span::SYNTHETIC)),
            // `Invalid` already carries a lex-time diagnostic; recover
            // without piling on a redundant "expected ';' or '$'".
            TokenKind::Invalid => {
                self.synchronise();
                None
            }
            _ => {
                let (_tok, span) = self.lexer.next();
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    span,
                    message: "expected ';' or '$' to end statement".to_string(),
                    code: DiagnosticCode::UnterminatedStatement,
                });
                self.synchronise();
                None
            }
        }
    }

    pub(crate) fn synchronise(&mut self) {
        let mut depth: u32 = 0;
        loop {
            match self.lexer.peek_kind() {
                TokenKind::LParen => { depth += 1; self.lexer.next(); }
                TokenKind::RParen if depth > 0 => { depth -= 1; self.lexer.next(); }
                TokenKind::Semi | TokenKind::Dollar if depth == 0 => {
                    self.lexer.next(); // consume terminator
                    break;
                }
                TokenKind::Eof => return,
                _ => { self.lexer.next(); }
            }
        }
    }

    fn skip_to_terminator(&mut self) {
        loop {
            match self.lexer.peek_kind() {
                TokenKind::Semi | TokenKind::Dollar => { self.lexer.next(); break; }
                TokenKind::Eof => break,
                _ => { self.lexer.next(); }
            }
        }
    }
}
