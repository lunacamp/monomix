use crate::expr::{ExprId, ExprPool};
use rustc_hash::FxHashMap;

/// Memoization cache for a single bottom-up substitution pass.
///
/// **Invariant — single binding set per cache.** Entries are keyed by
/// `ExprId` only; the binding `(var, value)` (or slice of bindings) is
/// *not* part of the key. Reusing the same cache across calls with
/// different bindings will return stale substitutions and silently
/// produce wrong results.
///
/// To enforce that invariant, the cached entry points
/// (`substitute_with_cache`, `substitute_many_with_cache`) are
/// `pub(crate)` — they're available to crate-internal callers who want
/// to amortise traversal cost across many roots under *one* binding set
/// (e.g. substituting `x → 5` into a batch of equations), but they
/// can't be reached from outside the kernel.
///
/// External callers should use [`substitute`] / [`substitute_many`],
/// which allocate a fresh cache per call — eliminating the foot-gun at
/// the cost of one `FxHashMap::default()` per top-level substitution.
pub(crate) type SubstituteCache = FxHashMap<ExprId, ExprId>;

/// Substitute `var → value` in `root`, bottom-up.
///
/// Allocates a fresh cache per call — safe to use anywhere, including
/// in loops with varying `var`/`value`.
pub fn substitute(
    pool: &mut ExprPool,
    root: ExprId,
    var: ExprId,
    value: ExprId,
) -> ExprId {
    substitute_many(pool, root, &[(var, value)])
}

/// Substitute all bindings in parallel — one bottom-up pass; replacements
/// are against the original expression, not cascading.
///
/// Allocates a fresh cache per call. For repeated substitutions with the
/// *same* bindings against many roots, see crate-internal
/// [`substitute_many_with_cache`].
pub fn substitute_many(
    pool: &mut ExprPool,
    root: ExprId,
    bindings: &[(ExprId, ExprId)],
) -> ExprId {
    let mut cache = SubstituteCache::default();
    substitute_many_with_cache(pool, &mut cache, root, bindings)
}

/// Cached form of [`substitute`]. **Crate-internal only** — the cache
/// is *not* keyed by `(var, value)`, so callers must ensure they only
/// reuse a given cache under one fixed binding set. Mixing bindings
/// silently returns stale results.
pub(crate) fn substitute_with_cache(
    pool: &mut ExprPool,
    cache: &mut SubstituteCache,
    root: ExprId,
    var: ExprId,
    value: ExprId,
) -> ExprId {
    substitute_many_with_cache(pool, cache, root, &[(var, value)])
}

/// Cached form of [`substitute_many`]. **Crate-internal only** — see
/// [`SubstituteCache`] for the cache-reuse contract. Use this to
/// amortise traversal across many roots that share a binding set.
pub(crate) fn substitute_many_with_cache(
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
        let result = substitute(&mut pool, expr, x, two);
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
        let result = substitute_many(&mut pool, expr, &[(x, a), (y, b)]);
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
        let result = substitute(&mut pool, eq, x, two);
        assert!(matches!(pool.get(result), crate::expr::ExprNode::Eq(_, _)));
    }

    #[test]
    fn substitute_back_to_back_with_different_bindings_is_safe() {
        // Regression: public `substitute` must isolate the cache per call.
        // Before this fix, the cached `substitute` was `pub` and reusing
        // one cache across calls with different `(var, value)` returned
        // stale results — there's no way to express that bug now because
        // the external API allocates internally, but we pin the contract
        // here so any future refactor that re-exposes a shared cache
        // would break this test.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let expr = pool.add(vec![x, x]); // x + x

        let r1 = substitute(&mut pool, expr, x, two);
        let r2 = substitute(&mut pool, expr, x, three);

        // r1 must contain 2, r2 must contain 3 — neither should still
        // contain `x`, and they must differ from each other.
        assert!(!pool.contains_symbol(r1, x));
        assert!(!pool.contains_symbol(r2, x));
        assert_ne!(r1, r2, "different bindings must produce different results");
    }

    #[test]
    fn substitute_with_cache_amortises_across_shared_bindings() {
        // Pin the crate-internal contract: the cached form works when the
        // binding set is held fixed across calls. This is the legitimate
        // use case the `pub(crate)` API exists for.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let two = pool.small_int(2);

        let e1 = pool.add(vec![x, y]);
        let e2 = pool.mul(vec![x, y]);

        let mut cache = SubstituteCache::default();
        let r1 = substitute_with_cache(&mut pool, &mut cache, e1, x, two);
        let r2 = substitute_with_cache(&mut pool, &mut cache, e2, x, two);

        // Both results have x replaced by 2; y is preserved.
        assert!(!pool.contains_symbol(r1, x));
        assert!(!pool.contains_symbol(r2, x));
        assert!(pool.contains_symbol(r1, y));
        assert!(pool.contains_symbol(r2, y));
        // The cache should have grown (x → 2 and the subexpr `x` are
        // shared between e1 and e2, so r2 reuses cached work).
        assert!(!cache.is_empty(), "second call should have populated cache");
    }
}
