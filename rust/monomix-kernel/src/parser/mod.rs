pub mod ast;
pub mod lexer;
pub(crate) mod expr;
pub(crate) mod stmt;

use crate::expr::ExprPool;
use crate::parser::ast::ParseResult;
use crate::parser::expr::{BuiltinIds, ExprParser};
use crate::parser::lexer::Lexer;
use rustc_hash::FxHashMap;

/// Parse `source` into a list of `Stmt`s, interning expressions into `pool`.
///
/// Never panics; all errors flow through `ParseResult.diagnostics`. Single
/// statements that fail to parse are dropped via `synchronise()`-based
/// recovery — subsequent statements are still attempted.
///
/// **Trailing `;`/`$` is optional.** If the final statement lacks a terminator,
/// it's accepted with `OutputMode::Display` and `Span::SYNTHETIC` for the
/// statement's terminator span. This mirrors REPL convention where the last
/// fragment may not have a trailing terminator yet. Embedded statements
/// (anywhere except the final one) DO require an explicit `;` or `$`; missing
/// terminators there emit `DiagnosticCode::UnterminatedStatement`.
pub fn parse(source: &str, pool: &mut ExprPool) -> ParseResult {
    let builtins = BuiltinIds::new(pool);
    let mut parser = ExprParser {
        lexer: Lexer::new(source),
        pool,
        diagnostics: Vec::new(),
        span_map: FxHashMap::default(),
        src: source,
        builtins,
    };
    let statements = parser.parse_program();
    ParseResult {
        statements,
        diagnostics: parser.diagnostics,
        span_map: parser.span_map,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;

    #[test]
    fn parse_single_statement_display() {
        let mut pool = ExprPool::new();
        let result = parse("1 + 2;", &mut pool);
        assert_eq!(result.statements.len(), 1);
        assert_eq!(result.diagnostics.len(), 0);
        assert_eq!(result.statements[0].output, crate::parser::ast::OutputMode::Display);
    }

    #[test]
    fn parse_suppress_with_dollar() {
        let mut pool = ExprPool::new();
        let result = parse("x + 1$", &mut pool);
        assert_eq!(result.statements[0].output, crate::parser::ast::OutputMode::Suppress);
    }

    #[test]
    fn parse_assignment() {
        let mut pool = ExprPool::new();
        let result = parse("y := 2 * x;", &mut pool);
        assert_eq!(result.statements.len(), 1);
        assert!(matches!(result.statements[0].kind, crate::parser::ast::StmtKind::Assign { .. }));
    }

    #[test]
    fn parse_multiple_statements() {
        let mut pool = ExprPool::new();
        let result = parse("a := 1; b := 2;", &mut pool);
        assert_eq!(result.statements.len(), 2);
    }

    #[test]
    fn parse_error_recovery() {
        let mut pool = ExprPool::new();
        let result = parse("1 + ; 2 + 3;", &mut pool);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.statements.len(), 1); // "2 + 3" parsed OK
    }
}
