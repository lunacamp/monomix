use crate::errors::{map_kernel_error, CrossSessionError};
use crate::expr::Expr;
use monomix_kernel::simplify::{SimplifierConfig, SimplifyCache};
use pyo3::prelude::*;
use std::sync::Arc;

#[pyfunction]
pub fn simplify(py: Python<'_>, e: &Expr) -> PyResult<Expr> {
    let pool_arc = Arc::clone(&e.pool);
    let original_id = e.id;
    let new_id = py.allow_threads(|| {
        let mut pool = pool_arc.lock().expect("pool mutex poisoned");
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        monomix_kernel::simplify::simplify(&mut pool, original_id, &config, &mut cache)
    });
    Ok(Expr::new(Arc::clone(&e.pool), new_id))
}

#[pyfunction]
pub fn df(py: Python<'_>, e: &Expr, x: &Expr) -> PyResult<Expr> {
    if !Arc::ptr_eq(&e.pool, &x.pool) {
        return Err(PyErr::new::<CrossSessionError, _>(
            "df: Expr and variable come from different Sessions",
        ));
    }
    let pool_arc = Arc::clone(&e.pool);
    let e_id = e.id;
    let x_id = x.id;
    let new_id = py.allow_threads(|| {
        let mut pool = pool_arc.lock().expect("pool mutex poisoned");
        monomix_kernel::differentiate(&mut pool, e_id, x_id)
    });
    new_id
        .map(|id| Expr::new(Arc::clone(&e.pool), id))
        .map_err(map_kernel_error)
}
