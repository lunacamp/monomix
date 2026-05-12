use arrayvec::ArrayVec;
use num_bigint::BigInt;
use crate::parser::ast::{Diagnostic, DiagnosticCode, Severity, Span, TokenKind};

// ---- Tokens ----------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum Token {
    SmallInt(i64),
    BigInt(Box<BigInt>),
    Float(f64),
    Ident(Span),

    Plus, Minus, Star, Slash,
    Pow,       // ^ or **
    Assign,    // :=
    Equals,    // =
    Comma, LParen, RParen,
    Semi, Dollar,
    KwComment,
    Invalid,   // lex error; lexer has already pushed a Diagnostic
    Eof,
}

impl Token {
    pub fn kind(&self) -> TokenKind {
        match self {
            Token::SmallInt(_) => TokenKind::SmallInt,
            Token::BigInt(_)   => TokenKind::BigInt,
            Token::Float(_)    => TokenKind::Float,
            Token::Ident(_)    => TokenKind::Ident,
            Token::Plus        => TokenKind::Plus,
            Token::Minus       => TokenKind::Minus,
            Token::Star        => TokenKind::Star,
            Token::Slash       => TokenKind::Slash,
            Token::Pow         => TokenKind::Pow,
            Token::Assign      => TokenKind::Assign,
            Token::Equals      => TokenKind::Equals,
            Token::Comma       => TokenKind::Comma,
            Token::LParen      => TokenKind::LParen,
            Token::RParen      => TokenKind::RParen,
            Token::Semi        => TokenKind::Semi,
            Token::Dollar      => TokenKind::Dollar,
            Token::KwComment   => TokenKind::KwComment,
            Token::Invalid     => TokenKind::Invalid,
            Token::Eof         => TokenKind::Eof,
        }
    }
}

// ---- Lexer -----------------------------------------------------------------

pub struct Lexer<'s> {
    src: &'s str,
    pos: usize,
    /// Two-slot lookahead. Slot 0 is the next token; slot 1 the after-next.
    /// Fed lazily on `peek_at(1)` calls.
    buffer: ArrayVec<(Token, Span), 2>,
    /// Diagnostics emitted during scanning. Drained by the parser.
    /// Every `Token::Invalid` produced by the lexer has a corresponding entry here.
    pub diagnostics: Vec<Diagnostic>,
}

impl<'s> Lexer<'s> {
    pub fn new(src: &'s str) -> Self {
        Lexer { src, pos: 0, buffer: ArrayVec::new(), diagnostics: Vec::new() }
    }

    fn push_diag(&mut self, span: Span, code: DiagnosticCode, message: String) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message,
            code,
        });
    }

    pub fn peek(&mut self) -> &(Token, Span) {
        if self.buffer.is_empty() {
            let tok = self.scan_next();
            self.buffer.push(tok);
        }
        &self.buffer[0]
    }

    /// Cheap kind-only peek — no payload clone. Used in the Pratt inner loop.
    pub fn peek_kind(&mut self) -> TokenKind {
        self.peek().0.kind()
    }

    /// Look at slot `offset` (0 or 1), filling intermediate slots as needed.
    ///
    /// **Precondition:** `offset <= 1`. The lookahead buffer has only two
    /// slots; an out-of-range offset would panic deep inside `ArrayVec::push`
    /// with an opaque message. We enforce the contract with an `assert!`
    /// (not `debug_assert!`) so release builds get the same clear failure
    /// mode as debug, and we expose the function as `pub(crate)` so an
    /// external user of the lexer can't reach the foot-gun at all. The
    /// only callers in this kernel are the assignment-detection check in
    /// the statement parser and a couple of two-token disambiguations in
    /// the expression parser, all using `offset == 1`.
    pub(crate) fn peek_at(&mut self, offset: usize) -> &(Token, Span) {
        assert!(offset <= 1, "Lexer lookahead is 2 slots; offset must be 0 or 1, got {offset}");
        while self.buffer.len() <= offset {
            let tok = self.scan_next();
            self.buffer.push(tok);
        }
        &self.buffer[offset]
    }

    /// Consume the next token. Drains slot 0; slot 1 (if present) shifts down.
    pub fn next(&mut self) -> (Token, Span) {
        if self.buffer.is_empty() {
            return self.scan_next();
        }
        self.buffer.remove(0)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip ASCII whitespace
            while self.pos < self.src.len()
                && self.src.as_bytes()[self.pos].is_ascii_whitespace()
            {
                self.pos += 1;
            }
            // Skip `% ...\n` line comments
            if self.pos < self.src.len() && self.src.as_bytes()[self.pos] == b'%' {
                while self.pos < self.src.len()
                    && self.src.as_bytes()[self.pos] != b'\n'
                {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
    }

    fn scan_next(&mut self) -> (Token, Span) {
        self.skip_whitespace_and_comments();

        if self.pos >= self.src.len() {
            let end = self.src.len() as u32;
            return (Token::Eof, Span { start: end, end });
        }

        let start = self.pos;
        let b = self.src.as_bytes()[self.pos];

        match b {
            b'+' => { self.pos += 1; (Token::Plus,   Span::of(start, self.pos)) }
            b'-' => { self.pos += 1; (Token::Minus,  Span::of(start, self.pos)) }
            b'*' => {
                if self.pos + 1 < self.src.len() && self.src.as_bytes()[self.pos + 1] == b'*' {
                    self.pos += 2;
                    (Token::Pow, Span::of(start, self.pos))
                } else {
                    self.pos += 1;
                    (Token::Star, Span::of(start, self.pos))
                }
            }
            b'/' => { self.pos += 1; (Token::Slash, Span::of(start, self.pos)) }
            b'^' => { self.pos += 1; (Token::Pow,   Span::of(start, self.pos)) }
            b'=' => { self.pos += 1; (Token::Equals, Span::of(start, self.pos)) }
            b',' => { self.pos += 1; (Token::Comma,  Span::of(start, self.pos)) }
            b'(' => { self.pos += 1; (Token::LParen, Span::of(start, self.pos)) }
            b')' => { self.pos += 1; (Token::RParen, Span::of(start, self.pos)) }
            b';' => { self.pos += 1; (Token::Semi,   Span::of(start, self.pos)) }
            b'$' => { self.pos += 1; (Token::Dollar, Span::of(start, self.pos)) }
            b':' => {
                if self.pos + 1 < self.src.len() && self.src.as_bytes()[self.pos + 1] == b'=' {
                    self.pos += 2;
                    (Token::Assign, Span::of(start, self.pos))
                } else {
                    self.pos += 1;
                    let span = Span::of(start, self.pos);
                    self.push_diag(
                        span,
                        DiagnosticCode::ExpectedAssignAfterColon,
                        "expected '=' after ':' (use ':=' for assignment)".to_string(),
                    );
                    (Token::Invalid, span)
                }
            }
            b'0'..=b'9' => self.scan_number(start),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.scan_ident(start),
            _ => {
                // Step over one full UTF-8 code point so we don't slice mid-char.
                let ch_len = utf8_char_len(b);
                let ch_end = (self.pos + ch_len).min(self.src.len());
                let bad = &self.src[self.pos..ch_end];
                self.pos = ch_end;
                let span = Span::of(start, self.pos);
                self.push_diag(
                    span,
                    DiagnosticCode::UnrecognizedCharacter,
                    format!("unrecognized character {:?}", bad),
                );
                (Token::Invalid, span)
            }
        }
    }



    fn scan_number(&mut self, start: usize) -> (Token, Span) {
        let src = self.src;
        while self.pos < src.len() && src.as_bytes()[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        let is_float = self.pos < src.len()
            && (src.as_bytes()[self.pos] == b'.'
                || src.as_bytes()[self.pos] == b'e'
                || src.as_bytes()[self.pos] == b'E');
        if is_float {
            if self.pos < src.len() && src.as_bytes()[self.pos] == b'.' {
                self.pos += 1;
                while self.pos < src.len() && src.as_bytes()[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
            }
            if self.pos < src.len()
                && (src.as_bytes()[self.pos] == b'e' || src.as_bytes()[self.pos] == b'E')
            {
                self.pos += 1;
                if self.pos < src.len()
                    && (src.as_bytes()[self.pos] == b'+' || src.as_bytes()[self.pos] == b'-')
                {
                    self.pos += 1;
                }
                while self.pos < src.len() && src.as_bytes()[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
            }
            let span = Span::of(start, self.pos);
            // 1024-byte cap on numeric literals
            if self.pos - start > 1024 {
                self.push_diag(
                    span,
                    DiagnosticCode::NumericLiteralTooLong,
                    "numeric literal exceeds 1024 bytes".to_string(),
                );
                return (Token::Invalid, span);
            }
            let s = &src[start..self.pos];
            match s.parse::<f64>() {
                Ok(f) if f.is_finite() => (Token::Float(f), span),
                // Reject inf / nan / overflow at lex time — never propagate
                // these into the kernel.
                Ok(_) | Err(_) => {
                    self.push_diag(
                        span,
                        DiagnosticCode::InvalidNumericLiteral,
                        "invalid float literal (overflow, NaN, or malformed)".to_string(),
                    );
                    (Token::Invalid, span)
                }
            }
        } else {
            let span = Span::of(start, self.pos);
            if self.pos - start > 1024 {
                self.push_diag(
                    span,
                    DiagnosticCode::NumericLiteralTooLong,
                    "numeric literal exceeds 1024 bytes".to_string(),
                );
                return (Token::Invalid, span);
            }
            let s = &src[start..self.pos];
            match s.parse::<i64>() {
                Ok(n) => (Token::SmallInt(n), span),
                Err(_) => match s.parse::<BigInt>() {
                    Ok(n) => (Token::BigInt(Box::new(n)), span),
                    Err(_) => {
                        self.push_diag(
                            span,
                            DiagnosticCode::InvalidNumericLiteral,
                            "invalid integer literal".to_string(),
                        );
                        (Token::Invalid, span)
                    }
                },
            }
        }
    }

    fn scan_ident(&mut self, start: usize) -> (Token, Span) {
        while self.pos < self.src.len() {
            let b = self.src.as_bytes()[self.pos];
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let span = Span::of(start, self.pos);
        // 1024-byte cap on identifier length
        if self.pos - start > 1024 {
            self.push_diag(
                span,
                DiagnosticCode::IdentifierTooLong,
                "identifier exceeds 1024 bytes".to_string(),
            );
            return (Token::Invalid, span);
        }
        let word = &self.src[start..self.pos];
        // `comment` keyword is case-insensitive
        if word.eq_ignore_ascii_case("comment") {
            return (Token::KwComment, span);
        }
        (Token::Ident(span), span)
    }
}

/// UTF-8 leading-byte width. Returns 1..=4 for valid leads, 1 as a safe
/// fallback for stray continuation bytes (we still advance one byte so
/// scanning makes progress).
fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 { 1 }
    else if b < 0xC0 { 1 }      // stray continuation byte
    else if b < 0xE0 { 2 }
    else if b < 0xF0 { 3 }
    else { 4 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::TokenKind;

    fn lex_all(src: &str) -> Vec<TokenKind> {
        let mut lexer = Lexer::new(src);
        let mut kinds = Vec::new();
        loop {
            let (tok, _) = lexer.next();
            let k = tok.kind();
            kinds.push(k);
            if k == TokenKind::Eof { break; }
        }
        kinds
    }

    #[test]
    fn lex_simple_expr() {
        assert_eq!(
            lex_all("1 + 2"),
            vec![TokenKind::SmallInt, TokenKind::Plus, TokenKind::SmallInt, TokenKind::Eof]
        );
    }

    #[test]
    fn lex_pow_both_spellings() {
        // Token stream: Ident(x), Pow(^), SmallInt(2), Plus, Ident(y), Pow(**), SmallInt(3), Eof
        let kinds = lex_all("x^2 + y**3");
        assert_eq!(kinds[1], TokenKind::Pow);
        assert_eq!(kinds[5], TokenKind::Pow);
    }

    #[test]
    fn lex_comment_stripped() {
        assert_eq!(
            lex_all("1 % this is a comment\n+ 2"),
            vec![TokenKind::SmallInt, TokenKind::Plus, TokenKind::SmallInt, TokenKind::Eof]
        );
    }

    #[test]
    fn lex_span_byte_accurate() {
        let src = "xy + 1";
        let mut lexer = Lexer::new(src);
        let (_, span) = lexer.next(); // "xy"
        assert_eq!(span.start, 0);
        assert_eq!(span.end, 2);
        assert_eq!(&src[span.start as usize..span.end as usize], "xy");
    }

    #[test]
    fn lex_inf_nan_as_ident() {
        // The lexer treats "inf" / "nan" as plain identifiers — float
        // literal rejection happens elsewhere (we never produce those f64s).
        let kinds = lex_all("inf");
        assert_eq!(kinds[0], TokenKind::Ident);
    }

    #[test]
    fn lex_assign_token() {
        assert_eq!(lex_all("x := 1")[1], TokenKind::Assign);
    }

    #[test]
    fn lex_peek_kind_no_clone() {
        let mut lexer = Lexer::new("1 + 2");
        assert_eq!(lexer.peek_kind(), TokenKind::SmallInt);
        assert_eq!(lexer.peek_kind(), TokenKind::SmallInt); // idempotent
        lexer.next(); // consume
        assert_eq!(lexer.peek_kind(), TokenKind::Plus);
    }

    #[test]
    fn lex_unrecognized_char_is_invalid_not_eof() {
        // Previously these collapsed to Eof and silently truncated parsing.
        let mut lexer = Lexer::new("1 ? 2");
        assert_eq!(lexer.next().0.kind(), TokenKind::SmallInt);
        assert_eq!(lexer.next().0.kind(), TokenKind::Invalid);
        assert_eq!(lexer.next().0.kind(), TokenKind::SmallInt);
        assert_eq!(lexer.next().0.kind(), TokenKind::Eof);
        assert_eq!(lexer.diagnostics.len(), 1);
        assert!(matches!(
            lexer.diagnostics[0].code,
            DiagnosticCode::UnrecognizedCharacter
        ));
    }

    #[test]
    fn lex_bare_colon_is_invalid_not_eof() {
        let mut lexer = Lexer::new("x : 1");
        assert_eq!(lexer.next().0.kind(), TokenKind::Ident);
        assert_eq!(lexer.next().0.kind(), TokenKind::Invalid);
        assert_eq!(lexer.next().0.kind(), TokenKind::SmallInt);
        assert_eq!(lexer.next().0.kind(), TokenKind::Eof);
        assert!(matches!(
            lexer.diagnostics[0].code,
            DiagnosticCode::ExpectedAssignAfterColon
        ));
    }

    #[test]
    fn lex_oversized_number_emits_diagnostic() {
        let huge = "1".repeat(2000);
        let mut lexer = Lexer::new(&huge);
        assert_eq!(lexer.next().0.kind(), TokenKind::Invalid);
        assert!(matches!(
            lexer.diagnostics[0].code,
            DiagnosticCode::NumericLiteralTooLong
        ));
    }

    #[test]
    fn lex_oversized_identifier_emits_diagnostic() {
        let huge = "a".repeat(2000);
        let mut lexer = Lexer::new(&huge);
        assert_eq!(lexer.next().0.kind(), TokenKind::Invalid);
        assert!(matches!(
            lexer.diagnostics[0].code,
            DiagnosticCode::IdentifierTooLong
        ));
    }

    #[test]
    fn lex_non_ascii_char_advances_full_codepoint() {
        // U+00E9 (é) is two bytes in UTF-8; we must not slice between them.
        let mut lexer = Lexer::new("é+1");
        assert_eq!(lexer.next().0.kind(), TokenKind::Invalid);
        assert_eq!(lexer.next().0.kind(), TokenKind::Plus);
        assert_eq!(lexer.next().0.kind(), TokenKind::SmallInt);
    }

    #[test]
    fn peek_at_within_two_slot_window_works() {
        // Happy path: offset 0 and 1 are both valid and stable across
        // repeat calls (the buffer is pre-populated lazily).
        let mut lexer = Lexer::new("1 + 2");
        assert_eq!(lexer.peek_at(0).0.kind(), TokenKind::SmallInt);
        assert_eq!(lexer.peek_at(1).0.kind(), TokenKind::Plus);
        // Repeating must be idempotent — must not over-fill the ArrayVec.
        assert_eq!(lexer.peek_at(0).0.kind(), TokenKind::SmallInt);
        assert_eq!(lexer.peek_at(1).0.kind(), TokenKind::Plus);
    }

    #[test]
    #[should_panic(expected = "Lexer lookahead is 2 slots")]
    fn peek_at_out_of_range_panics_with_clear_message() {
        // Regression: previously guarded only by `debug_assert!`, so
        // release builds would panic deep inside `ArrayVec::push` with
        // an opaque message ("vector overflow"). The upgraded `assert!`
        // surfaces the precondition violation cleanly in any build mode.
        let mut lexer = Lexer::new("1 + 2");
        let _ = lexer.peek_at(2);
    }
}
