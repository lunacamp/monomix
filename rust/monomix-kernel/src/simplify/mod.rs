pub mod driver;
pub mod like_terms;
pub mod numeric;
pub mod patterns;
pub mod powers;
pub mod rational;
pub mod rules;

pub use driver::{SimplifierConfig, SimplifyCache};

use crate::expr::{ExprId, ExprPool};
use crate::simplify::rules::DEFAULT_RULES;

pub fn simplify(
    pool: &mut ExprPool,
    expr: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
) -> ExprId {
    driver::simplify(pool, expr, config, cache, &DEFAULT_RULES)
}

pub fn simplify_trig(
    pool: &mut ExprPool,
    expr: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
) -> ExprId {
    let trig_reg = rules::trig_rules(pool);
    driver::simplify(pool, expr, config, cache, &trig_reg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ExprNode, ExprPool};

    #[test]
    fn simplify_x_plus_x_is_2x() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let expr = pool.add(vec![x, x]);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, expr, &config, &mut cache);
        let has_two = pool.fold(result, false, &mut |found, _id, node| {
            found || matches!(node, ExprNode::SmallInt(2))
        });
        assert!(has_two, "x + x should become 2*x");
    }

    #[test]
    fn simplify_idempotent() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let expr = pool.add(vec![x, x]);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let r1 = simplify(&mut pool, expr, &config, &mut cache);
        let r2 = simplify(&mut pool, r1, &config, &mut cache);
        assert_eq!(r1, r2, "simplify should be idempotent");
    }

    #[test]
    fn simplify_constant_fold_2_plus_3() {
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let expr = pool.add(vec![two, three]);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, expr, &config, &mut cache);
        assert_eq!(pool.get(result), &ExprNode::SmallInt(5));
    }

    #[test]
    fn simplify_x_mul_x_is_x_squared() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let expr = pool.mul(vec![x, x]);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, expr, &config, &mut cache);
        // x * x → x^2
        assert!(matches!(pool.get(result), ExprNode::Pow(_, _)));
    }
}
