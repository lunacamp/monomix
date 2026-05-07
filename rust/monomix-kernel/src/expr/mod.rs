// Task 4: ExprNode, ExprId, FnTag — core types only.

use num_bigint::BigInt;
use ordered_float::OrderedFloat;

// ---- Handles ---------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct LocalExprId(pub u32);

pub type ExprId = LocalExprId;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct InternedStr(pub u32);

// ---- Function tag ----------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FnTag {
    Sin,
    Cos,
    Tan,
    Exp,
    Log,
    Sqrt,
    Abs,
    Asin,
    Acos,
    Atan,
    Custom(InternedStr),
}

// ---- Expression node -------------------------------------------------------

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ExprNode {
    // Atoms
    SmallInt(i64),
    BigInt(Box<BigInt>),
    Rational(Box<(BigInt, BigInt)>),
    Float(OrderedFloat<f64>),
    Symbol(InternedStr),
    String(InternedStr),

    // Composite (children are immutable Box<[ExprId]>)
    Add(Box<[ExprId]>),
    Mul(Box<[ExprId]>),
    Pow(ExprId, ExprId),
    Neg(ExprId),
    Div(ExprId, ExprId),
    Eq(ExprId, ExprId),
    Fn(FnTag, Box<[ExprId]>),
    List(Box<[ExprId]>),
}

// Compile-time size guard — fails compilation if ExprNode size drifts.
// The runtime test (below) enforces the upper bound (<=32 bytes); this
// constant pins the *exact* current size so any future variant growth
// (or unintended shrink) is caught at build time.
const _EXPR_NODE_SIZE_GUARD: [(); 24] = [(); std::mem::size_of::<ExprNode>()];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expr_node_size_at_most_32_bytes() {
        // This is also enforced at compile time below, but we test it
        // explicitly so a failure has a clear test name.
        assert!(
            std::mem::size_of::<ExprNode>() <= 32,
            "ExprNode is {} bytes, must be <=32",
            std::mem::size_of::<ExprNode>()
        );
    }

    #[test]
    fn local_expr_id_is_copy() {
        let id = LocalExprId(0);
        let id2 = id; // copy
        assert_eq!(id, id2);
    }
}
