use monomix_kernel::KernelError;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

create_exception!(monomix._kernel, MonomixError, PyException);
create_exception!(monomix._kernel, ParseError, MonomixError);
create_exception!(monomix._kernel, EvalError, MonomixError);
create_exception!(monomix._kernel, UnsupportedError, MonomixError);
create_exception!(monomix._kernel, CrossSessionError, MonomixError);

pub fn map_kernel_error(err: KernelError) -> PyErr {
    match err {
        KernelError::Parse(diags) => {
            let msg = diags
                .iter()
                .map(|d| format!("{:?}", d))
                .collect::<Vec<_>>()
                .join("; ");
            PyErr::new::<ParseError, _>(msg)
        }
        KernelError::DivisionByZero { .. } => {
            PyErr::new::<EvalError, _>("division by zero")
        }
        KernelError::IndeterminateForm => {
            PyErr::new::<EvalError, _>("indeterminate form 0/0")
        }
        KernelError::UnboundSymbol(name) => {
            PyErr::new::<EvalError, _>(format!("unbound symbol: {}", name))
        }
        KernelError::LogOfNonPositive => {
            PyErr::new::<EvalError, _>("log of non-positive value")
        }
        KernelError::SqrtOfNegative => {
            PyErr::new::<EvalError, _>("sqrt of negative value")
        }
        KernelError::DomainError { fn_name } => {
            PyErr::new::<EvalError, _>(format!("domain error in {}", fn_name))
        }
        KernelError::UnsupportedFn => {
            PyErr::new::<UnsupportedError, _>("unsupported function for numeric eval")
        }
        KernelError::UnsupportedEquation { reason } => {
            PyErr::new::<UnsupportedError, _>(reason)
        }
        KernelError::SingularSystem => {
            PyErr::new::<EvalError, _>("singular system")
        }
        KernelError::Overflow => PyErr::new::<EvalError, _>("arithmetic overflow"),
        KernelError::NumericNaN => PyErr::new::<EvalError, _>("numeric evaluation produced NaN"),
        KernelError::DifferentiateEquation => {
            PyErr::new::<UnsupportedError, _>("cannot differentiate an equation")
        }
        KernelError::NotASymbol => {
            PyErr::new::<EvalError, _>("differentiation variable must be a symbol")
        }
        KernelError::SubstituteNotASymbol => {
            PyErr::new::<EvalError, _>("substitution target must be a symbol")
        }
        KernelError::CyclicBinding => {
            PyErr::new::<EvalError, _>("cyclic binding detected")
        }
        KernelError::PoolExhausted => {
            PyErr::new::<MonomixError, _>("expression pool exhausted")
        }
    }
}
