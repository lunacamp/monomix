use crate::expr::{ExprPool, ExprId, ExprNode, FnTag, InternedStr};
use crate::parser::ast::{Diagnostic, DiagnosticCode, Severity, Span, SpanMap, TokenKind};
use crate::parser::lexer::{Lexer, Token};

pub(crate) struct ExprParser<'s, 'p> {
    pub(crate) lexer: Lexer<'s>,
    pub(crate) pool: &'p mut ExprPool,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) span_map: SpanMap,
    pub(crate) src: &'s str,
    pub(crate) builtins: BuiltinIds,
}

#[derive(Clone, Copy)]
pub(crate) struct BuiltinIds {
    pub df:       InternedStr,
    pub int_:     InternedStr,
    pub solve:    InternedStr,
    pub factor:   InternedStr,
    pub expand:   InternedStr,
    pub simplify: InternedStr,
    pub sub:      InternedStr,
    // Built-in math functions — dispatched to their FnTag variants
    pub sin:  InternedStr,
    pub cos:  InternedStr,
    pub tan:  InternedStr,
    pub exp:  InternedStr,
    pub log:  InternedStr,
    pub sqrt: InternedStr,
    pub abs:  InternedStr,
    pub asin: InternedStr,
    pub acos: InternedStr,
    pub atan: InternedStr,
}

impl BuiltinIds {
    pub(crate) fn new(pool: &mut ExprPool) -> Self {
        BuiltinIds {
            df:       pool.intern_str_pub("df"),
            int_:     pool.intern_str_pub("int"),
            solve:    pool.intern_str_pub("solve"),
            factor:   pool.intern_str_pub("factor"),
            expand:   pool.intern_str_pub("expand"),
            simplify: pool.intern_str_pub("simplify"),
            sub:      pool.intern_str_pub("sub"),
            sin:      pool.intern_str_pub("sin"),
            cos:      pool.intern_str_pub("cos"),
            tan:      pool.intern_str_pub("tan"),
            exp:      pool.intern_str_pub("exp"),
            log:      pool.intern_str_pub("log"),
            sqrt:     pool.intern_str_pub("sqrt"),
            abs:      pool.intern_str_pub("abs"),
            asin:     pool.intern_str_pub("asin"),
            acos:     pool.intern_str_pub("acos"),
            atan:     pool.intern_str_pub("atan"),
        }
    }

    /// Map an interned name to a built-in FnTag variant, if any.
    pub(crate) fn fn_tag_for(&self, name: InternedStr) -> Option<FnTag> {
        if name == self.sin  { return Some(FnTag::Sin); }
        if name == self.cos  { return Some(FnTag::Cos); }
        if name == self.tan  { return Some(FnTag::Tan); }
        if name == self.exp  { return Some(FnTag::Exp); }
        if name == self.log  { return Some(FnTag::Log); }
        if name == self.sqrt { return Some(FnTag::Sqrt); }
        if name == self.abs  { return Some(FnTag::Abs); }
        if name == self.asin { return Some(FnTag::Asin); }
        if name == self.acos { return Some(FnTag::Acos); }
        if name == self.atan { return Some(FnTag::Atan); }
        None
    }
}

fn infix_bp(kind: TokenKind) -> Option<(u8, u8)> {
    match kind {
        TokenKind::Equals          => Some((10, 0)),    // non-associative
        TokenKind::Plus
        | TokenKind::Minus         => Some((20, 21)),   // left-assoc
        TokenKind::Star
        | TokenKind::Slash         => Some((30, 31)),   // left-assoc
        TokenKind::Pow             => Some((50, 49)),   // right-assoc
        _                          => None,
    }
}

fn prefix_bp(kind: TokenKind) -> Option<u8> {
    match kind {
        TokenKind::Minus => Some(40),
        _                => None,
    }
}

impl<'s, 'p> ExprParser<'s, 'p> {
    pub(crate) fn parse_expr(&mut self, min_bp: u8) -> Result<ExprId, ()> {
        // Boundary tokens (Semi, Dollar, Eof, RParen) are not expression
        // starters. If we encounter one in prefix position, emit a diagnostic
        // WITHOUT consuming — leave the token for the caller (e.g. statement
        // synchronisation) to handle. This keeps error recovery accurate so
        // a malformed expression doesn't eat the trailing terminator.
        //
        // `Invalid` already carries a lexer-emitted diagnostic; bail to
        // synchronise without re-diagnosing.
        match self.lexer.peek_kind() {
            TokenKind::Invalid => return Err(()),
            TokenKind::Semi | TokenKind::Dollar | TokenKind::Eof | TokenKind::RParen => {
                let span = self.lexer.peek().1;
                let found = self.lexer.peek_kind();
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    span,
                    message: format!("unexpected token {:?}, expected expression", found),
                    code: DiagnosticCode::UnexpectedToken { found, expected: "expression" },
                });
                return Err(());
            }
            _ => {}
        }
        let (tok, tok_span) = self.lexer.next();
        let mut lhs = match tok {
            Token::SmallInt(n) => {
                let id = self.pool.small_int(n);
                self.span_map.insert(id, tok_span);
                id
            }
            Token::BigInt(n) => {
                let id = self.pool.integer(*n);
                self.span_map.insert(id, tok_span);
                id
            }
            Token::Float(f) => {
                let id = self.pool.float(f);
                self.span_map.insert(id, tok_span);
                id
            }
            Token::Ident(span) => self.parse_ident_or_call(span)?,
            Token::LParen => {
                let inner = self.parse_expr(0)?;
                self.expect(TokenKind::RParen, "')'")?;
                inner
            }
            Token::Minus => {
                let bp = prefix_bp(TokenKind::Minus).unwrap();
                let right = self.parse_expr(bp)?;
                let id = self.pool.neg(right);
                self.span_map.insert(id, tok_span);
                id
            }
            other => {
                self.emit_unexpected(other, tok_span, "expression");
                return Err(());
            }
        };

        // Rational literal shortcut: SmallInt/BigInt '/' SmallInt/BigInt
        if matches!(self.pool.get(lhs), ExprNode::SmallInt(_) | ExprNode::BigInt(_))
            && self.lexer.peek_kind() == TokenKind::Slash
        {
            if let Some(rat_id) = self.try_rational(lhs) {
                lhs = rat_id;
            }
        }

        // Pratt infix loop — kind-only peek, no clone.
        loop {
            let op_kind = self.lexer.peek_kind();
            let Some((left_bp, right_bp)) = infix_bp(op_kind) else { break };
            if left_bp <= min_bp { break }
            let (op_tok, _op_span) = self.lexer.next();
            let rhs = self.parse_expr(right_bp)?;
            lhs = self.build_infix(op_tok, lhs, rhs);
        }

        Ok(lhs)
    }

    fn try_rational(&mut self, lhs: ExprId) -> Option<ExprId> {
        use num_traits::Zero;
        if self.lexer.peek_at(0).0.kind() != TokenKind::Slash {
            return None;
        }

        // Validate the denominator at slot 1 (without consuming). Must be an
        // integer literal — SmallInt or BigInt — and not zero. Inspecting by
        // reference avoids moving the Box<BigInt> out of the lexer buffer;
        // the actual value is extracted after both tokens are consumed.
        match self.lexer.peek_at(1).0.kind() {
            TokenKind::SmallInt => {
                if matches!(self.lexer.peek_at(1).0, Token::SmallInt(0)) {
                    return None;
                }
            }
            TokenKind::BigInt => {
                // The lexer narrows numeric literals to SmallInt when they
                // fit in i64, so `Token::BigInt(0)` is unreachable from
                // valid source — but check defensively in case that ever
                // changes (and to make the invariant local to this fn).
                if let Token::BigInt(b) = &self.lexer.peek_at(1).0 {
                    if b.is_zero() {
                        return None;
                    }
                }
            }
            _ => return None,
        }

        // Numerator (LHS): SmallInt or BigInt only.
        let p: num_bigint::BigInt = match self.pool.get(lhs).clone() {
            ExprNode::SmallInt(p) => num_bigint::BigInt::from(p),
            ExprNode::BigInt(p) => *p,
            _ => return None,
        };

        // Commit: consume the '/' and the denominator, then extract.
        self.lexer.next();
        let (den_tok, _) = self.lexer.next();
        let q: num_bigint::BigInt = match den_tok {
            Token::SmallInt(q) => num_bigint::BigInt::from(q),
            Token::BigInt(b) => *b,
            _ => unreachable!("guarded by the kind/zero peek above"),
        };

        Some(self.pool.rational(p, q))
    }

    fn build_infix(&mut self, op: Token, lhs: ExprId, rhs: ExprId) -> ExprId {
        match op {
            Token::Plus    => self.pool.add(vec![lhs, rhs]),
            Token::Minus   => {
                let neg = self.pool.neg(rhs);
                self.pool.add(vec![lhs, neg])
            }
            Token::Star    => self.pool.mul(vec![lhs, rhs]),
            Token::Slash   => self.pool.div(lhs, rhs),
            Token::Pow     => self.pool.pow(lhs, rhs),
            Token::Equals  => self.pool.eq_node(lhs, rhs),
            _ => unreachable!("non-infix operator passed to build_infix"),
        }
    }

    fn parse_ident_or_call(&mut self, ident_span: Span) -> Result<ExprId, ()> {
        let raw = &self.src[ident_span.start as usize..ident_span.end as usize];
        // `intern_str_pub` lowercases internally — pass the raw slice to
        // avoid a redundant `to_lowercase()` allocation on every identifier.
        let name = self.pool.intern_str_pub(raw);

        if self.lexer.peek_kind() != TokenKind::LParen {
            let id = self.pool.symbol_by_id(name);
            self.span_map.insert(id, ident_span);
            return Ok(id);
        }
        self.lexer.next(); // consume '('

        let bt = self.builtins;
        if name == bt.df       { return self.parse_df(); }
        if name == bt.int_     { return self.parse_unary_builtin(FnTag::Custom(name)); }
        if name == bt.solve    { return self.parse_solve_call(); }
        if name == bt.factor   { return self.parse_unary_builtin(FnTag::Custom(name)); }
        if name == bt.expand   { return self.parse_unary_builtin(FnTag::Custom(name)); }
        if name == bt.simplify { return self.parse_unary_builtin(FnTag::Custom(name)); }
        if name == bt.sub      { return self.parse_sub(); }
        // Math built-ins (sin/cos/etc.) → proper FnTag variant
        if let Some(tag) = bt.fn_tag_for(name) {
            let args = self.parse_arg_list()?;
            return Ok(self.pool.func(tag, args));
        }
        self.parse_generic_call(name)
    }

    fn parse_arg_list(&mut self) -> Result<Vec<ExprId>, ()> {
        let mut args = Vec::new();
        if self.lexer.peek_kind() == TokenKind::RParen {
            self.lexer.next();
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr(0)?);
            match self.lexer.peek_kind() {
                TokenKind::Comma => { self.lexer.next(); }
                TokenKind::RParen => { self.lexer.next(); break; }
                _ => {
                    let (tok, span) = self.lexer.next();
                    self.emit_unexpected(tok, span, "',' or ')'");
                    return Err(());
                }
            }
        }
        Ok(args)
    }

    fn parse_generic_call(&mut self, name: InternedStr) -> Result<ExprId, ()> {
        let args = self.parse_arg_list()?;
        Ok(self.pool.func(FnTag::Custom(name), args))
    }

    fn parse_unary_builtin(&mut self, tag: FnTag) -> Result<ExprId, ()> {
        let arg = self.parse_expr(0)?;
        self.expect(TokenKind::RParen, "')'")?;
        Ok(self.pool.func(tag, vec![arg]))
    }

    fn parse_df(&mut self) -> Result<ExprId, ()> {
        let expr = self.parse_expr(0)?;
        self.expect(TokenKind::Comma, "','")?;
        let var = self.parse_expr(0)?;
        let mut result = self.pool.func(FnTag::Custom(self.builtins.df), vec![expr, var]);
        while self.lexer.peek_kind() == TokenKind::Comma {
            self.lexer.next();
            let next_var = self.parse_expr(0)?;
            result = self.pool.func(FnTag::Custom(self.builtins.df), vec![result, next_var]);
        }
        self.expect(TokenKind::RParen, "')'")?;
        Ok(result)
    }

    fn parse_solve_call(&mut self) -> Result<ExprId, ()> {
        let eq = self.parse_expr(0)?;
        self.expect(TokenKind::Comma, "','")?;
        let var = self.parse_expr(0)?;
        self.expect(TokenKind::RParen, "')'")?;
        Ok(self.pool.func(FnTag::Custom(self.builtins.solve), vec![eq, var]))
    }

    fn parse_sub(&mut self) -> Result<ExprId, ()> {
        // sub(x = val, expr) or sub(x = a, y = b, expr)
        let mut bindings: Vec<ExprId> = Vec::new();
        loop {
            let lhs = self.parse_expr(0)?;
            self.expect(TokenKind::Equals, "'=' in sub binding")?;
            let rhs = self.parse_expr(0)?;
            bindings.push(self.pool.eq_node(lhs, rhs));
            match self.lexer.peek_kind() {
                TokenKind::Comma => {
                    self.lexer.next();
                    // If next two tokens look like another binding (IDENT '='), continue
                    if self.lexer.peek_kind() == TokenKind::Ident
                        && self.lexer.peek_at(1).0.kind() == TokenKind::Equals
                    {
                        continue;
                    }
                    // Otherwise treat the next expression as the target
                    let target = self.parse_expr(0)?;
                    self.expect(TokenKind::RParen, "')'")?;
                    let mut args = bindings;
                    args.push(target);
                    return Ok(self.pool.func(FnTag::Custom(self.builtins.sub), args));
                }
                _ => {
                    let (tok, span) = self.lexer.next();
                    self.emit_unexpected(tok, span, "',' before sub() target expression");
                    return Err(());
                }
            }
        }
    }

    pub(crate) fn expect(&mut self, kind: TokenKind, expected: &'static str) -> Result<(Token, Span), ()> {
        let (tok, span) = self.lexer.next();
        if tok.kind() == kind {
            Ok((tok, span))
        } else {
            self.emit_unexpected(tok, span, expected);
            Err(())
        }
    }

    fn emit_unexpected(&mut self, tok: Token, span: Span, expected: &'static str) {
        let found = tok.kind();
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message: format!("unexpected token {:?}, expected {}", found, expected),
            code: DiagnosticCode::UnexpectedToken { found, expected },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;
    use crate::parser::lexer::Lexer;

    fn parse_one(src: &str) -> (ExprPool, crate::expr::ExprId) {
        let mut pool = ExprPool::new();
        let builtins = BuiltinIds::new(&mut pool);
        let mut p = ExprParser {
            lexer: Lexer::new(src),
            pool: &mut pool,
            diagnostics: Vec::new(),
            span_map: rustc_hash::FxHashMap::default(),
            src,
            builtins,
        };
        let id = p.parse_expr(0).expect("parse should succeed");
        assert!(p.diagnostics.is_empty(), "diagnostics: {:?}", p.diagnostics);
        let id_copy = id;
        drop(p);
        (pool, id_copy)
    }

    #[test]
    fn parse_integer_literal() {
        let (pool, id) = parse_one("42");
        assert_eq!(*pool.get(id), crate::expr::ExprNode::SmallInt(42));
    }

    #[test]
    fn parse_precedence_add_mul() {
        let (pool, id) = parse_one("1 + 2 * 3");
        if let crate::expr::ExprNode::Add(children) = pool.get(id).clone() {
            assert_eq!(children.len(), 2);
            let mul = pool.get(children[1]).clone();
            assert!(matches!(mul, crate::expr::ExprNode::Mul(_)));
        } else {
            panic!("expected Add, got {:?}", pool.get(id));
        }
    }

    #[test]
    fn parse_pow_right_associative() {
        let (pool, id) = parse_one("2^3^4");
        if let crate::expr::ExprNode::Pow(_, exp) = *pool.get(id) {
            assert!(matches!(pool.get(exp), crate::expr::ExprNode::Pow(_, _)));
        } else {
            panic!("expected Pow, got {:?}", pool.get(id));
        }
    }

    #[test]
    fn parse_double_negation_normalizes() {
        let (pool, id) = parse_one("-(-x)");
        assert!(matches!(pool.get(id), crate::expr::ExprNode::Symbol(_)));
    }

    #[test]
    fn parse_equality() {
        let (pool, id) = parse_one("x = y");
        assert!(matches!(pool.get(id), crate::expr::ExprNode::Eq(_, _)));
    }

    #[test]
    fn parse_sin_dispatches_to_fn_sin() {
        let (pool, id) = parse_one("sin(x)");
        match pool.get(id) {
            crate::expr::ExprNode::Fn(crate::expr::FnTag::Sin, args) => {
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Fn(Sin, ...), got {:?}", other),
        }
    }

    #[test]
    fn parse_log_dispatches_to_fn_log() {
        let (pool, id) = parse_one("log(x)");
        assert!(matches!(pool.get(id), crate::expr::ExprNode::Fn(crate::expr::FnTag::Log, _)));
    }

    #[test]
    fn parse_unknown_function_is_custom() {
        let (pool, id) = parse_one("foo(x, y)");
        match pool.get(id) {
            crate::expr::ExprNode::Fn(crate::expr::FnTag::Custom(_), args) => {
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected Fn(Custom(...), ...), got {:?}", other),
        }
    }

    // ---- try_rational with BigInt operands -----------------------------

    #[test]
    fn parse_rational_with_bigint_numerator() {
        // 10^20 / 3 — numerator exceeds i64::MAX, forces BigInt token.
        // Before this fix, try_rational's SmallInt-only LHS arm returned
        // None and parsing fell back to Div(BigInt, SmallInt).
        let (pool, id) = parse_one("100000000000000000000/3");
        match pool.get(id) {
            crate::expr::ExprNode::Rational(b) => {
                // gcd(10^20, 3) = 1, so the rational is already in lowest terms.
                assert_eq!(b.0.to_string(), "100000000000000000000");
                assert_eq!(b.1.to_string(), "3");
            }
            other => panic!("expected Rational(10^20, 3), got {:?}", other),
        }
    }

    #[test]
    fn parse_rational_with_bigint_denominator() {
        let (pool, id) = parse_one("1/100000000000000000000");
        match pool.get(id) {
            crate::expr::ExprNode::Rational(b) => {
                assert_eq!(b.0.to_string(), "1");
                assert_eq!(b.1.to_string(), "100000000000000000000");
            }
            other => panic!("expected Rational(1, 10^20), got {:?}", other),
        }
    }

    #[test]
    fn parse_rational_with_bigint_both_sides() {
        // 10^20 / (2 * 10^20) reduces to 1/2 after pool.rational's
        // gcd normalization — confirming the BigInt path goes through
        // canonicalization, not just `Div`.
        let (pool, id) = parse_one("100000000000000000000/200000000000000000000");
        match pool.get(id) {
            crate::expr::ExprNode::Rational(b) => {
                assert_eq!(b.0.to_string(), "1");
                assert_eq!(b.1.to_string(), "2");
            }
            other => panic!("expected Rational(1, 2), got {:?}", other),
        }
    }

    #[test]
    fn parse_rational_smallint_zero_denominator_falls_back_to_div() {
        // 1/0 must still leave the parser intact and not panic. The
        // earlier zero-denominator fix established this contract; this
        // test pins it down so future try_rational rewrites don't regress.
        let (pool, id) = parse_one("1/0");
        // Falls back to the generic Pratt infix path → Div(1, 0).
        assert!(matches!(pool.get(id), crate::expr::ExprNode::Div(_, _)));
    }
}
