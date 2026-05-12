use crate::parser::ast::Span;

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    // Parser
    #[error("parse error")]
    Parse(Vec<crate::parser::ast::Diagnostic>),

    // Expression pool
    #[error("pool exhausted")]
    PoolExhausted,
    #[error("division by zero")]
    DivisionByZero { span: Option<Span> },
    #[error("indeterminate form 0/0")]
    IndeterminateForm,

    // Differentiator
    #[error("cannot differentiate an equation")]
    DifferentiateEquation,
    #[error("differentiation variable must be a symbol")]
    NotASymbol,

    // Substitution
    #[error("substitution target must be a symbol")]
    SubstituteNotASymbol,
    #[error("cyclic binding detected")]
    CyclicBinding,

    // Numeric evaluation
    #[error("unbound symbol: {0}")]
    UnboundSymbol(String),
    #[error("log of non-positive value")]
    LogOfNonPositive,
    #[error("sqrt of negative value")]
    SqrtOfNegative,
    #[error("domain error in {fn_name}")]
    DomainError { fn_name: &'static str },
    #[error("unsupported function for numeric eval")]
    UnsupportedFn,

    // Solver
    #[error("unsupported equation form: {reason}")]
    UnsupportedEquation { reason: String },
    #[error("singular system")]
    SingularSystem,

    // Arithmetic
    #[error("arithmetic overflow")]
    Overflow,
    #[error("numeric evaluation produced NaN")]
    NumericNaN,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_division_by_zero() {
        let e = KernelError::DivisionByZero { span: None };
        assert_eq!(e.to_string(), "division by zero");
    }

    #[test]
    fn error_display_unbound_symbol() {
        let e = KernelError::UnboundSymbol("x".to_string());
        assert_eq!(e.to_string(), "unbound symbol: x");
    }

    #[test]
    fn error_display_unsupported_equation() {
        let e = KernelError::UnsupportedEquation { reason: "cubic".to_string() };
        assert_eq!(e.to_string(), "unsupported equation form: cubic");
    }
}
