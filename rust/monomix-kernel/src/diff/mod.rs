pub mod arith;
pub mod driver;
pub mod functions;
pub mod plugin;
pub mod table;

use crate::expr::{ExprId, ExprPool};
use crate::error::KernelError;
use crate::diff::driver::{DiffCache, diff_impl};

/// Differentiate `expr` with respect to `var`. Per-call DiffCache.
pub fn differentiate(
    pool: &mut ExprPool,
    expr: ExprId,
    var: ExprId,
) -> Result<ExprId, KernelError> {
    if !matches!(pool.get(var), crate::expr::ExprNode::Symbol(_)) {
        return Err(KernelError::NotASymbol);
    }
    let mut cache: DiffCache = DiffCache::default();
    diff_impl(pool, expr, var, &mut cache)
}

pub fn differentiate_with_cache(
    pool: &mut ExprPool,
    expr: ExprId,
    var: ExprId,
    cache: &mut DiffCache,
) -> Result<ExprId, KernelError> {
    if !matches!(pool.get(var), crate::expr::ExprNode::Symbol(_)) {
        return Err(KernelError::NotASymbol);
    }
    diff_impl(pool, expr, var, cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ExprNode, ExprPool};

    #[test]
    fn diff_symbol_wrt_self_is_one() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let result = differentiate(&mut pool, x, x).unwrap();
        assert_eq!(result, pool.one);
    }

    #[test]
    fn diff_symbol_wrt_other_is_zero() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let result = differentiate(&mut pool, y, x).unwrap();
        assert_eq!(result, pool.zero);
    }

    #[test]
    fn diff_constant_is_zero() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let five = pool.small_int(5);
        let result = differentiate(&mut pool, five, x).unwrap();
        assert_eq!(result, pool.zero);
    }

    #[test]
    fn diff_x_squared_is_2x() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let result = differentiate(&mut pool, x2, x).unwrap();
        // Should be 2*x
        let has_two = pool.fold(result, false, &mut |found, _id, node| {
            found || matches!(node, ExprNode::SmallInt(2))
        });
        assert!(has_two, "d/dx x^2 should produce 2 as coefficient");
    }

    #[test]
    fn diff_sum_linearity() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two_int = pool.small_int(2);
        let three_int = pool.small_int(3);
        let x2 = pool.pow(x, two_int);
        let x3 = pool.pow(x, three_int);
        let sum = pool.add(vec![x2, x3]);
        let result = differentiate(&mut pool, sum, x).unwrap();
        // d/dx(x^2 + x^3) = 2x + 3x^2 — should be an Add
        assert!(matches!(pool.get(result), ExprNode::Add(_)));
    }

    #[test]
    fn diff_sin_x_is_cos_x() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let sin_x = pool.func(crate::expr::FnTag::Sin, vec![x]);
        let result = differentiate(&mut pool, sin_x, x).unwrap();
        assert!(matches!(pool.get(result), ExprNode::Fn(crate::expr::FnTag::Cos, _)));
    }

    #[test]
    fn diff_eq_raises_error() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        let eq = pool.eq_node(x, one);
        let result = differentiate(&mut pool, eq, x);
        assert!(matches!(result, Err(crate::error::KernelError::DifferentiateEquation)));
    }

    #[test]
    fn diff_cache_per_call() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let r1 = differentiate(&mut pool, x2, x).unwrap();
        let r2 = differentiate(&mut pool, x2, x).unwrap();
        assert_eq!(r1, r2);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::expr::ExprPool;
    use crate::simplify::{simplify, SimplifierConfig, SimplifyCache};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn diff_linearity(a in 1i64..10, b in 1i64..10) {
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let a_int = pool.small_int(a);
            let b_int = pool.small_int(b);
            let ax = pool.mul(vec![a_int, x]);
            let bx = pool.mul(vec![b_int, x]);
            let sum = pool.add(vec![ax, bx]);
            // d/dx(a*x + b*x) should equal (a+b)
            let d = differentiate(&mut pool, sum, x).unwrap();
            let config = SimplifierConfig::default();
            let mut cache = SimplifyCache::new();
            let simplified = simplify(&mut pool, d, &config, &mut cache);
            let total = a + b;
            let has_total = pool.fold(simplified, false, &mut |found, _id, node| {
                found || matches!(node, crate::expr::ExprNode::SmallInt(n) if *n == total)
            });
            prop_assert!(has_total, "d/dx((a+b)*x) should produce {}", total);
        }

        #[test]
        fn diff_leibniz_product_rule(n in 2u32..6u32) {
            // d/dx(x^n) = n*x^(n-1)
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let n_int = pool.small_int(n as i64);
            let xn = pool.pow(x, n_int);
            let d = differentiate(&mut pool, xn, x).unwrap();
            let has_n = pool.fold(d, false, &mut |found, _id, node| {
                found || matches!(node, crate::expr::ExprNode::SmallInt(k) if *k == n as i64)
            });
            prop_assert!(has_n, "d/dx x^{} should contain {} as coefficient", n, n);
        }
    }
}
