use monomix_kernel::ExprPool;
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
}

impl SessionHandle {
    pub fn pool_clone(&self) -> Arc<Mutex<ExprPool>> {
        Arc::clone(&self.pool)
    }
}
