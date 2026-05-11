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

    #[test]
    fn simplify_constant_fold_subtraction_via_parser() {
        // Regression: the parser lowers `10 - 3` to `Add([10, Neg(3)])`,
        // and previously `fold_numeric` bailed because `Neg(_)` wasn't
        // recognized as a numeric atom inside the accumulation loop.
        let mut pool = ExprPool::new();
        let result = crate::parser::parse("10 - 3;", &mut pool);
        assert_eq!(result.diagnostics.len(), 0);
        assert_eq!(result.statements.len(), 1);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let folded = simplify(&mut pool, result.statements[0].expr, &config, &mut cache);
        assert_eq!(pool.get(folded), &ExprNode::SmallInt(7));
    }

    #[test]
    fn simplify_constant_fold_neg_factor_via_parser() {
        // `2 * -3` lowers through Pratt's prefix Minus into `Mul([2, Neg(3)])`.
        let mut pool = ExprPool::new();
        let result = crate::parser::parse("2 * -3;", &mut pool);
        assert_eq!(result.diagnostics.len(), 0);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let folded = simplify(&mut pool, result.statements[0].expr, &config, &mut cache);
        assert_eq!(pool.get(folded), &ExprNode::SmallInt(-6));
    }

    #[test]
    fn simplify_cache_invalidates_when_rule_registry_changes() {
        // Regression: SimplifyCache used to be keyed only by ExprId, so
        // calling simplify() (DEFAULT_RULES, no trig rewrites) and then
        // simplify_trig() with the same cache returned the stale cached
        // result and silently skipped the Pythagorean rewrite.
        use crate::expr::FnTag;

        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let sin_x = pool.func(FnTag::Sin, vec![x]);
        let cos_x = pool.func(FnTag::Cos, vec![x]);
        let two = pool.small_int(2);
        let sin_sq = pool.pow(sin_x, two);
        let cos_sq = pool.pow(cos_x, two);
        let expr = pool.add(vec![sin_sq, cos_sq]);

        let cfg = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();

        // Step 1 — DEFAULT_RULES has no trig identities, so the expression
        // round-trips through the simplifier unchanged. This warms the
        // cache with `expr → expr`.
        let r1 = simplify(&mut pool, expr, &cfg, &mut cache);
        assert_eq!(r1, expr, "DEFAULT_RULES should not collapse sin^2 + cos^2");

        // Step 2 — same cache, switch to trig registry. The Pythagorean
        // rule must fire and reduce the expression to 1. Pre-fix, the
        // stale `expr → expr` mapping shadowed the rule and r2 == expr.
        let r2 = simplify_trig(&mut pool, expr, &cfg, &mut cache);
        assert_eq!(
            pool.get(r2),
            &ExprNode::SmallInt(1),
            "simplify_trig must reduce sin^2 + cos^2 to 1, not return the \
             stale DEFAULT_RULES cache entry; got {:?}",
            pool.get(r2)
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::expr::ExprPool;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn simplify_idempotent_arbitrary(n in 1i64..100, m in 1i64..100) {
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let n_int = pool.small_int(n);
            let m_int = pool.small_int(m);
            let nx = pool.mul(vec![n_int, x]);
            let mx = pool.mul(vec![m_int, x]);
            let expr = pool.add(vec![nx, mx]);
            let config = SimplifierConfig::default();
            let mut cache = SimplifyCache::new();
            let r1 = simplify(&mut pool, expr, &config, &mut cache);
            let r2 = simplify(&mut pool, r1, &config, &mut cache);
            prop_assert_eq!(r1, r2, "simplify must be idempotent");
        }

        #[test]
        fn simplify_iters_at_most_2(n in 1i64..20, m in 1i64..20) {
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let n_int = pool.small_int(n);
            let m_int = pool.small_int(m);
            let nx = pool.mul(vec![n_int, x]);
            let mx = pool.mul(vec![m_int, x]);
            let expr = pool.add(vec![nx, mx]);
            let config = SimplifierConfig::default();
            let mut cache = SimplifyCache::new();
            let mut iters = 0u32;
            let mut current = expr;
            for _ in 0..driver::MAX_ITERS {
                let next = {
                    let mut map_cache = rustc_hash::FxHashMap::default();
                    pool.map_bottom_up(current, &mut map_cache, &mut |pool, id| {
                        driver::simplify_node_public(pool, id, &config, &mut cache)
                    })
                };
                iters += 1;
                if next == current { break; }
                current = next;
            }
            prop_assert!(iters <= 2, "should converge in <=2 iterations for Phase 1 rule set, got {}", iters);
        }
    }
}
