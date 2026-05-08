pub mod diff;
pub mod error;
pub mod expr;
pub mod parser;
pub mod poly;
pub mod simplify;

pub use diff::differentiate;
pub use error::KernelError;
pub use expr::{ExprId, ExprNode, ExprPool, FnTag, InternedStr, LocalExprId};
pub use parser::{parse, ast::ParseResult};
