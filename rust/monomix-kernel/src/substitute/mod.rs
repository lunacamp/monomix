use crate::expr::{ExprId, ExprPool};
use rustc_hash::FxHashMap;

pub type SubstituteCache = FxHashMap<ExprId, ExprId>;

/// Substitute `var → value` in `root`, bottom-up.
pub fn substitute(
    pool: &mut ExprPool,
    cache: &mut SubstituteCache,
    root: ExprId,
    var: ExprId,
    value: ExprId,
) -> ExprId {
    substitute_many(pool, cache, root, &[(var, value)])
}

/// Substitute all bindings in parallel — one bottom-up pass; replacements
/// are against the original expression, not cascading.
pub fn substitute_many(
    pool: &mut ExprPool,
    cache: &mut SubstituteCache,
    root: ExprId,
    bindings: &[(ExprId, ExprId)],
) -> ExprId {
    pool.map_bottom_up(root, cache, &mut |_pool, id| {
        for &(var, val) in bindings {
            if id == var {
                return val;
            }
        }
        id
    })
}

/// Convenience wrapper: creates a fresh cache for one-shot use.
pub fn substitute_fresh(
    pool: &mut ExprPool,
    root: ExprId,
    var: ExprId,
    value: ExprId,
) -> ExprId {
    let mut cache = SubstituteCache::default();
    substitute(pool, &mut cache, root, var, value)
}

pub fn substitute_many_fresh(
    pool: &mut ExprPool,
    root: ExprId,
    bindings: &[(ExprId, ExprId)],
) -> ExprId {
    let mut cache = SubstituteCache::default();
    substitute_many(pool, &mut cache, root, bindings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;

    #[test]
    fn substitute_symbol_replaces() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let expr = pool.mul(vec![x, x]); // x*x
        let mut cache = SubstituteCache::default();
        let result = substitute(&mut pool, &mut cache, expr, x, two);
        // After substitution, x should not appear
        let has_x = pool.contains_symbol(result, x);
        assert!(!has_x, "x should be replaced");
    }

    #[test]
    fn substitute_many_parallel() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let a = pool.small_int(3);
        let b = pool.small_int(4);
        let expr = pool.add(vec![x, y]);
        let mut cache = SubstituteCache::default();
        let result = substitute_many(&mut pool, &mut cache, expr, &[(x, a), (y, b)]);
        // Result should not have x or y
        assert!(!pool.contains_symbol(result, x));
        assert!(!pool.contains_symbol(result, y));
    }

    #[test]
    fn substitute_eq_componentwise() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        let two = pool.small_int(2);
        let eq = pool.eq_node(x, one);
        let mut cache = SubstituteCache::default();
        let result = substitute(&mut pool, &mut cache, eq, x, two);
        assert!(matches!(pool.get(result), crate::expr::ExprNode::Eq(_, _)));
    }
}
