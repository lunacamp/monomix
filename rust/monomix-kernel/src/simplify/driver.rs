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

#[derive(Default)]
pub struct SimplifyCache(pub FxHashMap<ExprId, ExprId>);

impl SimplifyCache {
    pub fn new() -> Self { Self::default() }
    const EVICT_THRESHOLD: usize = 100_000;
    pub fn maybe_evict(&mut self) {
        if self.0.len() > Self::EVICT_THRESHOLD {
            self.0.clear();
        }
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
    if let Some(&cached) = cache.0.get(&expr) {
        return cached;
    }
    let result = simplify_node_inner(pool, expr, config, rules);
    cache.0.insert(expr, result);
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
        ExprNode::Pow(_, _) => consolidate_nested_pow(pool, expr),
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
