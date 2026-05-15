use crate::errors::{map_kernel_error, CrossSessionError};
use crate::expr::Expr;
use monomix_kernel::simplify::{SimplifierConfig, SimplifyCache};
use monomix_kernel::ExprId;
use pyo3::prelude::*;
use pyo3::types::PyDict;
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

#[pyfunction]
pub fn expand(py: Python<'_>, e: &Expr) -> PyResult<Expr> {
    let pool_arc = Arc::clone(&e.pool);
    let e_id = e.id;
    let new_id = py.allow_threads(|| {
        let mut pool = pool_arc.lock().expect("pool mutex poisoned");
        monomix_kernel::poly::expand(&mut pool, e_id)
    });
    Ok(Expr::new(Arc::clone(&e.pool), new_id))
}

#[pyfunction]
pub fn solve(py: Python<'_>, eq: &Expr, x: &Expr) -> PyResult<Vec<Expr>> {
    if !Arc::ptr_eq(&eq.pool, &x.pool) {
        return Err(PyErr::new::<CrossSessionError, _>(
            "solve: arguments from different Sessions",
        ));
    }
    let pool_arc = Arc::clone(&eq.pool);
    let eq_id = eq.id;
    let x_id = x.id;
    let result = py.allow_threads(|| {
        let mut pool = pool_arc.lock().expect("pool mutex poisoned");
        monomix_kernel::solve::solve(&mut pool, eq_id, x_id)
    });
    let sset = result.map_err(map_kernel_error)?;
    let pool_clone = Arc::clone(&eq.pool);
    let values = sset
        .solutions
        .into_iter()
        .flat_map(|subst| {
            subst
                .into_iter()
                .map(|(_var, val)| Expr::new(Arc::clone(&pool_clone), val))
        })
        .collect();
    Ok(values)
}

#[pyfunction]
pub fn sub(py: Python<'_>, mapping: &Bound<'_, PyDict>, e: &Expr) -> PyResult<Expr> {
    let pool_arc = Arc::clone(&e.pool);
    let mut pairs: Vec<(ExprId, ExprId)> = Vec::with_capacity(mapping.len());
    for (k, v) in mapping.iter() {
        let k_expr: PyRef<Expr> = k.extract()?;
        let v_expr: PyRef<Expr> = v.extract()?;
        if !Arc::ptr_eq(&k_expr.pool, &pool_arc) || !Arc::ptr_eq(&v_expr.pool, &pool_arc) {
            return Err(PyErr::new::<CrossSessionError, _>(
                "sub: mapping contains Expr from a different Session",
            ));
        }
        pairs.push((k_expr.id, v_expr.id));
    }
    let e_id = e.id;
    let new_id = py.allow_threads(|| {
        let mut pool = pool_arc.lock().expect("pool mutex poisoned");
        monomix_kernel::substitute::substitute_many(&mut pool, e_id, &pairs)
    });
    Ok(Expr::new(Arc::clone(&e.pool), new_id))
}

#[pyfunction]
pub fn evaluate_numeric(py: Python<'_>, e: &Expr) -> PyResult<f64> {
    let pool_arc = Arc::clone(&e.pool);
    let e_id = e.id;
    let result = py.allow_threads(|| {
        let pool = pool_arc.lock().expect("pool mutex poisoned");
        monomix_kernel::evalnum::evaluate_numeric(&pool, &[], e_id)
    });
    result.map_err(map_kernel_error)
}
