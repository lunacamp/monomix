use crate::expr::{ExprId, ExprNode, ExprPool};
use crate::simplify::like_terms::collect_like_terms;
use crate::simplify::numeric::fold_numeric;
use crate::simplify::patterns::RuleRegistry;
use crate::simplify::powers::{consolidate_nested_pow, consolidate_powers};
use crate::simplify::rational::simplify_div;
use rustc_hash::FxHashMap;

pub const MAX_ITERS: usize = 3;

#[derive(Debug, Clone)]
pub struct SimplifierConfig {
    pub gcd: bool,
    pub expand_powers: bool,
}

impl Default for SimplifierConfig {
    fn default() -> Self {
        SimplifierConfig { gcd: false, expand_powers: false }
    }
}

/// Memoization cache for `simplify_node` lookups.
///
/// **Rule-set awareness.** Cached `(input → output)` mappings are only sound
/// under the rule registry that produced them: a result computed with
/// `DEFAULT_RULES` (empty) is *not* equivalent to what the trig registry
/// would have produced for the same input. To make this sound, entries are
/// keyed by `(registry_addr, ExprId)` — the registry's address (as `usize`)
/// is part of the key, so distinct registries see disjoint cache partitions
/// and can never serve each other's stale results.
///
/// **Perf note.** Identity is by pointer (as `usize`), so registries with
/// stable addresses (`DEFAULT_RULES` is a `LazyLock`) get persistent cache
/// reuse across calls. `simplify_trig`, in contrast, currently builds a
/// fresh `RuleRegistry` per call, so each call gets a fresh stack address
/// and its own cache partition. That's correctness-safe (a stale entry can
/// never be returned) but it does mean repeated `simplify_trig` calls don't
/// benefit from cache reuse — callers who want that should hoist the
/// registry construction and call `driver::simplify` directly.
#[derive(Default)]
pub struct SimplifyCache {
    map: FxHashMap<(usize, ExprId), ExprId>,
}

impl SimplifyCache {
    pub fn new() -> Self { Self::default() }

    const EVICT_THRESHOLD: usize = 100_000;

    pub fn maybe_evict(&mut self) {
        if self.map.len() > Self::EVICT_THRESHOLD {
            self.map.clear();
        }
    }

    /// Number of cached `(registry, input) → output` entries. Exposed for
    /// tests and bench harnesses; not part of the simplification contract.
    pub fn len(&self) -> usize { self.map.len() }

    pub fn is_empty(&self) -> bool { self.map.is_empty() }

    #[inline]
    fn lookup(&self, rules: &RuleRegistry, expr: ExprId) -> Option<ExprId> {
        let addr = rules as *const RuleRegistry as usize;
        self.map.get(&(addr, expr)).copied()
    }

    #[inline]
    fn store(&mut self, rules: &RuleRegistry, expr: ExprId, result: ExprId) {
        let addr = rules as *const RuleRegistry as usize;
        self.map.insert((addr, expr), result);
    }
}

/// Simplify, bottom-up, up to MAX_ITERS fixed-point iterations.
pub fn simplify(
    pool: &mut ExprPool,
    root: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
    rules: &RuleRegistry,
) -> ExprId {
    cache.maybe_evict();
    let mut current = root;
    for _ in 0..MAX_ITERS {
        let next = simplify_pass(pool, current, config, cache, rules);
        if next == current { break; }
        current = next;
    }
    current
}

fn simplify_pass(
    pool: &mut ExprPool,
    root: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
    rules: &RuleRegistry,
) -> ExprId {
    let mut map_cache = FxHashMap::default();
    pool.map_bottom_up(root, &mut map_cache, &mut |pool, id| {
        simplify_node(pool, id, config, cache, rules)
    })
}

fn simplify_node(
    pool: &mut ExprPool,
    expr: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
    rules: &RuleRegistry,
) -> ExprId {
    if let Some(cached) = cache.lookup(rules, expr) {
        return cached;
    }
    let result = simplify_node_inner(pool, expr, config, rules);
    cache.store(rules, expr, result);
    result
}

/// Public entry to single-node simplification under DEFAULT_RULES.
/// Used by the proptest in `simplify::proptests` to manually count iters.
pub fn simplify_node_public(
    pool: &mut ExprPool,
    expr: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
) -> ExprId {
    simplify_node(pool, expr, config, cache, &crate::simplify::rules::DEFAULT_RULES)
}

fn simplify_node_inner(
    pool: &mut ExprPool,
    expr: ExprId,
    config: &SimplifierConfig,
    rules: &RuleRegistry,
) -> ExprId {
    // 1. Try the rule registry first.
    if let Some(result) = rules.apply(pool, expr) {
        return result;
    }

    match pool.get(expr).clone() {
        ExprNode::Add(_) => {
            if let Some(folded) = fold_numeric(pool, expr) {
                return folded;
            }
            collect_like_terms(pool, expr)
        }
        ExprNode::Mul(_) => {
            if let Some(folded) = fold_numeric(pool, expr) {
                return folded;
            }
            consolidate_powers(pool, expr)
        }
        ExprNode::Pow(_, _) => {
            // Try numeric folding first (e.g. `2^10 → 1024`); fold_numeric
            // already handles SmallInt^SmallInt with non-negative exponent.
            // Falls through to nested-pow consolidation for symbolic bases.
            if let Some(folded) = fold_numeric(pool, expr) {
                return folded;
            }
            consolidate_nested_pow(pool, expr)
        }
        ExprNode::Div(_, _) => {
            if config.gcd {
                simplify_div(pool, expr).unwrap_or(expr)
            } else {
                expr
            }
        }
        _ => expr,
    }
}
