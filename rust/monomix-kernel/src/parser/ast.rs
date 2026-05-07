#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span { pub start: u32, pub end: u32 }

#[derive(Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub message: String,
    pub code: DiagnosticCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity { Error, Warning }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    UnexpectedToken { found: TokenKind, expected: &'static str },
    UnterminatedStatement,
    UnbalancedParen,
    InvalidNumericLiteral,
    NumericLiteralTooLong,
    IdentifierTooLong,
    MissingArgument { function: &'static str },
    TooManyArguments { function: &'static str, max: usize },
}

// forward ref — real definition in lexer.rs (Task 9)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    SmallInt, BigInt, Float, Ident,
    Plus, Minus, Star, Slash, Pow,
    Assign, Equals, Comma, LParen, RParen,
    Semi, Dollar, KwComment, Eof,
}
