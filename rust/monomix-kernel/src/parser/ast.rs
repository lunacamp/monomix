use crate::expr::ExprId;
use rustc_hash::FxHashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span { pub start: u32, pub end: u32 }

#[derive(Debug, Clone)]
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
    UnrecognizedCharacter,
    ExpectedAssignAfterColon,
    MissingArgument { function: &'static str },
    TooManyArguments { function: &'static str, max: usize },
}

// forward ref — real definition in lexer.rs (Task 9)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    SmallInt, BigInt, Float, Ident,
    Plus, Minus, Star, Slash, Pow,
    Assign, Equals, Comma, LParen, RParen,
    Semi, Dollar, KwComment, Invalid, Eof,
}

impl Span {
    pub const SYNTHETIC: Span = Span { start: u32::MAX, end: u32::MAX };

    pub fn of(start: usize, end: usize) -> Self {
        Span { start: start as u32, end: end as u32 }
    }

    pub fn merge(self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }

    pub fn to_str<'s>(&self, source: &'s str) -> &'s str {
        if self.start == u32::MAX { return "<synthetic>"; }
        &source[self.start as usize..self.end as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputMode { Display, Suppress }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StmtKind {
    Expr,
    Assign { lhs: crate::expr::InternedStr },
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub expr: ExprId,
    pub output: OutputMode,
    pub span: Span,
}

pub type SpanMap = FxHashMap<ExprId, Span>;

#[derive(Debug)]
pub struct ParseResult {
    pub statements: Vec<Stmt>,
    pub diagnostics: Vec<Diagnostic>,
    pub span_map: SpanMap,
}
