use crate::errors::CrossSessionError;
use monomix_kernel::{ExprId, ExprNode, ExprPool};
use num_bigint::BigInt;
use pyo3::prelude::*;
use pyo3::types::PyAny;
use std::sync::{Arc, Mutex};

fn coerce_to_expr(
    value: &Bound<'_, PyAny>,
    pool: &Arc<Mutex<ExprPool>>,
) -> PyResult<Expr> {
    if let Ok(e) = value.extract::<PyRef<Expr>>() {
        if !Arc::ptr_eq(&e.pool, pool) {
            return Err(PyErr::new::<CrossSessionError, _>(
                "Expr objects come from different Sessions",
            ));
        }
        return Ok(Expr::new(Arc::clone(pool), e.id));
    }
    if let Ok(n) = value.extract::<BigInt>() {
        let mut p = pool.lock().expect("pool mutex poisoned");
        let id = p.integer(n);
        return Ok(Expr::new(Arc::clone(pool), id));
    }
    if let Ok(f) = value.extract::<f64>() {
        let mut p = pool.lock().expect("pool mutex poisoned");
        let id = p.float(f);
        return Ok(Expr::new(Arc::clone(pool), id));
    }
    Err(pyo3::exceptions::PyTypeError::new_err(
        "operand must be Expr, int, or float",
    ))
}

#[pyclass(name = "Expr", module = "monomix._kernel", frozen)]
pub struct Expr {
    pub pool: Arc<Mutex<ExprPool>>,
    pub id: ExprId,
}

impl Expr {
    pub fn new(pool: Arc<Mutex<ExprPool>>, id: ExprId) -> Self {
        Expr { pool, id }
    }

    /// Returns Err with a CrossSessionError if `other` belongs to a different pool.
    pub fn require_same_pool(&self, other: &Expr) -> PyResult<()> {
        if Arc::ptr_eq(&self.pool, &other.pool) {
            Ok(())
        } else {
            Err(PyErr::new::<CrossSessionError, _>(
                "Expr objects come from different Sessions",
            ))
        }
    }
}

#[pymethods]
impl Expr {
    fn __repr__(&self) -> String {
        let pool = self.pool.lock().expect("pool mutex poisoned");
        format!("Expr({})", render_node(&pool, self.id))
    }

    fn is_same(&self, other: &Expr) -> bool {
        Arc::ptr_eq(&self.pool, &other.pool) && self.id == other.id
    }

    #[getter]
    fn kind(&self) -> &'static str {
        let pool = self.pool.lock().expect("pool mutex poisoned");
        match pool.get(self.id) {
            ExprNode::SmallInt(_) => "SmallInt",
            ExprNode::BigInt(_) => "BigInt",
            ExprNode::Rational(_) => "Rational",
            ExprNode::Float(_) => "Float",
            ExprNode::Symbol(_) => "Symbol",
            ExprNode::String(_) => "String",
            ExprNode::Add(_) => "Add",
            ExprNode::Mul(_) => "Mul",
            ExprNode::Pow(_, _) => "Pow",
            ExprNode::Neg(_) => "Neg",
            ExprNode::Div(_, _) => "Div",
            ExprNode::Eq(_, _) => "Eq",
            ExprNode::Fn(_, _) => "Fn",
            ExprNode::List(_) => "List",
            ExprNode::Lt(_, _) => "Lt",
            ExprNode::Le(_, _) => "Le",
            ExprNode::Gt(_, _) => "Gt",
            ExprNode::Ge(_, _) => "Ge",
            ExprNode::Not(_) => "Not",
            ExprNode::And(_) => "And",
            ExprNode::Or(_) => "Or",
            ExprNode::Implies(_, _) => "Implies",
            ExprNode::BoolConst(_) => "BoolConst",
        }
    }

    fn __add__(&self, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let rhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.add(vec![self.id, rhs.id]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __radd__(&self, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let lhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.add(vec![lhs.id, self.id]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __sub__(&self, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let rhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let neg_b = pool.neg(rhs.id);
        let id = pool.add(vec![self.id, neg_b]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __rsub__(&self, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let lhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let neg_self = pool.neg(self.id);
        let id = pool.add(vec![lhs.id, neg_self]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __mul__(&self, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let rhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.mul(vec![self.id, rhs.id]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __rmul__(&self, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let lhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.mul(vec![lhs.id, self.id]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __truediv__(&self, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let rhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.div(self.id, rhs.id);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __rtruediv__(&self, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let lhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.div(lhs.id, self.id);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __pow__(
        &self,
        other: &Bound<'_, PyAny>,
        _modulo: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Expr> {
        let rhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.pow(self.id, rhs.id);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __rpow__(
        &self,
        other: &Bound<'_, PyAny>,
        _modulo: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Expr> {
        let lhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.pow(lhs.id, self.id);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __neg__(&self) -> Expr {
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.neg(self.id);
        Expr::new(Arc::clone(&self.pool), id)
    }

    fn __pos__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let rhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.eq_node(self.id, rhs.id);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __ne__(&self, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let rhs = coerce_to_expr(other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let eq = pool.eq_node(self.id, rhs.id);
        let id = pool.not_node(eq);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __bool__(&self) -> PyResult<bool> {
        let pool = self.pool.lock().expect("pool mutex poisoned");
        match pool.get(self.id) {
            ExprNode::Eq(a, b) => Ok(a == b),
            ExprNode::Not(inner) => match pool.get(*inner) {
                ExprNode::Eq(a, b) => Ok(a != b),
                _ => Err(pyo3::exceptions::PyTypeError::new_err(
                    "ambiguous truth value of symbolic expression — \
                     use is_same() or evaluate first",
                )),
            },
            ExprNode::BoolConst(b) => Ok(*b),
            _ => Err(pyo3::exceptions::PyTypeError::new_err(
                "ambiguous truth value of symbolic expression — \
                 use is_same() or evaluate first",
            )),
        }
    }

    fn __hash__(&self) -> u64 {
        self.id.0 as u64
    }
}

fn render_node(pool: &ExprPool, id: ExprId) -> String {
    match pool.get(id) {
        ExprNode::SmallInt(n) => n.to_string(),
        ExprNode::BigInt(b) => b.to_string(),
        ExprNode::Rational(r) => format!("{}/{}", r.0, r.1),
        ExprNode::Float(f) => f.into_inner().to_string(),
        ExprNode::Symbol(s) => pool.str_of(*s).to_string(),
        ExprNode::String(s) => format!("\"{}\"", pool.str_of(*s)),
        ExprNode::BoolConst(b) => b.to_string(),
        _ => format!("<{:?}>", id),
    }
}
