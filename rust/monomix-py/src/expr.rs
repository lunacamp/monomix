use crate::errors::CrossSessionError;
use monomix_kernel::{ExprId, ExprNode, ExprPool};
use pyo3::prelude::*;
use std::sync::{Arc, Mutex};

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
