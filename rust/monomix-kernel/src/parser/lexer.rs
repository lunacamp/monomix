use arrayvec::ArrayVec;
use num_bigint::BigInt;
use crate::parser::ast::{Span, TokenKind};

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
}

impl<'s> Lexer<'s> {
    pub fn new(src: &'s str) -> Self {
        Lexer { src, pos: 0, buffer: ArrayVec::new() }
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

    /// Look at slot `offset` (0 or 1), filling intermediate slots.
    pub fn peek_at(&mut self, offset: usize) -> &(Token, Span) {
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
                    (Token::Eof, Span::of(start, self.pos))
                }
            }
            b'0'..=b'9' => self.scan_number(start),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.scan_ident(start),
            _ => {
                self.pos += 1;
                (Token::Eof, Span::of(start, self.pos))
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
                return (Token::Eof, span);
            }
            let s = &src[start..self.pos];
            match s.parse::<f64>() {
                Ok(f) if f.is_finite() => (Token::Float(f), span),
                // Reject inf / nan / overflow at lex time — never propagate
                // these into the kernel.
                Ok(_) | Err(_) => (Token::Eof, span),
            }
        } else {
            let span = Span::of(start, self.pos);
            if self.pos - start > 1024 {
                return (Token::Eof, span);
            }
            let s = &src[start..self.pos];
            match s.parse::<i64>() {
                Ok(n) => (Token::SmallInt(n), span),
                Err(_) => match s.parse::<BigInt>() {
                    Ok(n) => (Token::BigInt(Box::new(n)), span),
                    Err(_) => (Token::Eof, span),
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
            return (Token::Eof, span);
        }
        let word = &self.src[start..self.pos];
        // `comment` keyword is case-insensitive
        if word.eq_ignore_ascii_case("comment") {
            return (Token::KwComment, span);
        }
        (Token::Ident(span), span)
    }
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
}
