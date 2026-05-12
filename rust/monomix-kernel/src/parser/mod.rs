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
    // Merge lex diagnostics with parser diagnostics, then sort by span start
    // so the user sees errors in source order regardless of which layer emitted.
    let mut diagnostics = parser.lexer.diagnostics;
    diagnostics.append(&mut parser.diagnostics);
    diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
    ParseResult {
        statements,
        diagnostics,
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

    #[test]
    fn parse_comment_block_skipped() {
        let mut pool = ExprPool::new();
        let result = parse("comment foo bar baz; 1 + 2;", &mut pool);
        // The comment block consumes itself; only `1 + 2;` survives.
        assert_eq!(result.diagnostics.len(), 0);
        assert_eq!(result.statements.len(), 1);
    }

    #[test]
    fn parse_nested_paren_recovery() {
        let mut pool = ExprPool::new();
        let result = parse("(1 + ); 2 + 3;", &mut pool);
        // The malformed first stmt produces >=1 diagnostic; the second parses.
        assert!(!result.diagnostics.is_empty());
        assert_eq!(result.statements.len(), 1);
    }

    #[test]
    fn parse_unrecognized_char_does_not_truncate_program() {
        // Regression: the lexer used to fabricate Eof for unrecognized bytes,
        // silently dropping every statement that followed.
        let mut pool = ExprPool::new();
        let result = parse("? 1 + 2; 3 + 4;", &mut pool);
        assert_eq!(result.statements.len(), 2, "trailing statements must still parse");
        assert!(
            result.diagnostics.iter().any(|d| matches!(
                d.code,
                crate::parser::ast::DiagnosticCode::UnrecognizedCharacter
            )),
            "expected an UnrecognizedCharacter diagnostic"
        );
    }

    #[test]
    fn parse_bare_colon_does_not_truncate_program() {
        // Regression: typo `:` (instead of `:=`) used to swallow the rest of input.
        let mut pool = ExprPool::new();
        let result = parse("x : 1; y := 2;", &mut pool);
        assert!(
            result.statements.iter().any(|s| matches!(
                s.kind,
                crate::parser::ast::StmtKind::Assign { .. }
            )),
            "the `y := 2` statement after the malformed colon must still parse"
        );
        assert!(
            result.diagnostics.iter().any(|d| matches!(
                d.code,
                crate::parser::ast::DiagnosticCode::ExpectedAssignAfterColon
            )),
            "expected an ExpectedAssignAfterColon diagnostic"
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::expr::ExprPool;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn no_panic_on_arbitrary_input(s in "[ -~]{0,200}") {
            let mut pool = ExprPool::new();
            let result = parse(&s, &mut pool);
            // Should never panic; diagnostics or stmts may be anything
            // (We use len() >= 0 as a no-op assertion that the result is well-formed.)
            let _ = result.diagnostics.len() + result.statements.len();
        }

        #[test]
        fn span_bounds_valid(s in "[a-z0-9 +*();]{0,100}") {
            let mut pool = ExprPool::new();
            let result = parse(&s, &mut pool);
            for (_, span) in &result.span_map {
                prop_assert!(span.start <= span.end);
                prop_assert!((span.end as usize) <= s.len()
                    || *span == crate::parser::ast::Span::SYNTHETIC);
            }
        }
    }
}
