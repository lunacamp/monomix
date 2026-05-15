use pyo3::prelude::*;
use pyo3::wrap_pyfunction;

mod errors;
mod expr;
mod kernel_fns;
mod session;

#[pymodule]
fn _kernel(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("MonomixError", m.py().get_type_bound::<errors::MonomixError>())?;
    m.add("ParseError", m.py().get_type_bound::<errors::ParseError>())?;
    m.add("EvalError", m.py().get_type_bound::<errors::EvalError>())?;
    m.add("UnsupportedError", m.py().get_type_bound::<errors::UnsupportedError>())?;
    m.add("CrossSessionError", m.py().get_type_bound::<errors::CrossSessionError>())?;
    m.add_class::<session::SessionHandle>()?;
    m.add_class::<expr::Expr>()?;
    m.add_function(wrap_pyfunction!(kernel_fns::simplify, m)?)?;
    m.add_function(wrap_pyfunction!(kernel_fns::df, m)?)?;
    Ok(())
}
