use crate::errors::{map_kernel_error, ParseError};
use crate::expr::Expr;
use monomix_kernel::parser::ast::Severity;
use monomix_kernel::{ExprPool, KernelError};
use num_bigint::BigInt;
use pyo3::prelude::*;
use std::sync::{Arc, Mutex};

#[pyclass(name = "_SessionHandle", module = "monomix._kernel")]
pub struct SessionHandle {
    pub pool: Arc<Mutex<ExprPool>>,
}

#[pymethods]
impl SessionHandle {
    #[new]
    fn new() -> Self {
        SessionHandle {
            pool: Arc::new(Mutex::new(ExprPool::new())),
        }
    }

    fn symbol(&self, name: &str) -> Expr {
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.symbol(name);
        Expr::new(Arc::clone(&self.pool), id)
    }

    fn integer(&self, n: BigInt) -> Expr {
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.integer(n);
        Expr::new(Arc::clone(&self.pool), id)
    }

    fn rational(&self, p: BigInt, q: BigInt) -> PyResult<Expr> {
        use num_traits::Zero;
        if q.is_zero() {
            return Err(map_kernel_error(KernelError::DivisionByZero { span: None }));
        }
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.rational(p, q);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn parse(&self, py: Python<'_>, source: &str) -> PyResult<Expr> {
        let pool_arc = Arc::clone(&self.pool);
        let parse_result = py.allow_threads(|| {
            let mut pool = pool_arc.lock().expect("pool mutex poisoned");
            monomix_kernel::parse(source, &mut pool)
        });
        let has_errors = parse_result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error);
        if has_errors {
            return Err(map_kernel_error(KernelError::Parse(parse_result.diagnostics)));
        }
        let stmts = parse_result.statements;
        match stmts.len() {
            0 => Err(PyErr::new::<ParseError, _>(
                "empty input — no statements parsed",
            )),
            1 => Ok(Expr::new(Arc::clone(&self.pool), stmts[0].expr)),
            n => Err(PyErr::new::<ParseError, _>(format!(
                "parse() expects a single expression; got {} statements — \
                 pass each one separately",
                n
            ))),
        }
    }
}

impl SessionHandle {
    pub fn pool_clone(&self) -> Arc<Mutex<ExprPool>> {
        Arc::clone(&self.pool)
    }
}
