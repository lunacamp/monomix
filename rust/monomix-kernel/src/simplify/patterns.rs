use crate::expr::{ExprId, ExprNode, ExprPool, FnTag, InternedStr};
use rustc_hash::FxHashMap;

/// A metavariable matches any subexpression.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MetaVar(pub InternedStr);

/// Pattern for left-hand side matching.
#[derive(Clone, Debug)]
pub enum Pattern {
    /// Match any expression, binding to this MetaVar.
    Any(MetaVar),
    /// Match an exact ExprId (constant in the pool).
    Exact(ExprId),
    /// Match Add([Pattern, ...]). Matches in any order (commutative).
    Add(Vec<Pattern>),
    /// Match Mul([Pattern, ...]). Matches in any order (commutative).
    Mul(Vec<Pattern>),
    /// Match Pow(base_pattern, exp_pattern).
    Pow(Box<Pattern>, Box<Pattern>),
    /// Match Fn(tag, [Pattern, ...]).
    Fn(FnTag, Vec<Pattern>),
}

pub type MatchEnv = FxHashMap<MetaVar, ExprId>;

impl Pattern {
    pub fn matches(&self, pool: &ExprPool, expr: ExprId, env: &mut MatchEnv) -> bool {
        match self {
            Pattern::Any(mv) => {
                if let Some(&existing) = env.get(mv) {
                    existing == expr
                } else {
                    env.insert(*mv, expr);
                    true
                }
            }
            Pattern::Exact(id) => *id == expr,
            Pattern::Fn(tag, args) => {
                if let ExprNode::Fn(t, a) = pool.get(expr) {
                    if t != tag || a.len() != args.len() { return false; }
                    let a_ids: Vec<ExprId> = a.to_vec();
                    args.iter().zip(a_ids.iter()).all(|(p, &e)| p.matches(pool, e, env))
                } else { false }
            }
            Pattern::Pow(bp, ep) => {
                if let ExprNode::Pow(b, e) = pool.get(expr) {
                    let (b, e) = (*b, *e);
                    bp.matches(pool, b, env) && ep.matches(pool, e, env)
                } else { false }
            }
            Pattern::Add(pats) => match_commutative(pool, expr, pats, env, true),
            Pattern::Mul(pats) => match_commutative(pool, expr, pats, env, false),
        }
    }
}

/// Try to match `pats` against the children of `expr` (Add or Mul) in any
/// order. Phase 1: only succeeds if `pats.len() == expr's child count`,
/// using a brute-force permutation search backed by environment snapshots.
/// Acceptable because Phase 1 patterns are tiny (Pythagorean has 2 children).
fn match_commutative(
    pool: &ExprPool,
    expr: ExprId,
    pats: &[Pattern],
    env: &mut MatchEnv,
    is_add: bool,
) -> bool {
    let children: Vec<ExprId> = match pool.get(expr) {
        ExprNode::Add(c) if is_add  => c.to_vec(),
        ExprNode::Mul(c) if !is_add => c.to_vec(),
        _ => return false,
    };
    if children.len() != pats.len() {
        return false;
    }
    let mut used = vec![false; children.len()];
    try_permute(pool, pats, &children, &mut used, env)
}

fn try_permute(
    pool: &ExprPool,
    pats: &[Pattern],
    children: &[ExprId],
    used: &mut [bool],
    env: &mut MatchEnv,
) -> bool {
    if pats.is_empty() { return true; }
    let head = &pats[0];
    let rest = &pats[1..];
    for i in 0..children.len() {
        if used[i] { continue; }
        let snapshot = env.clone();
        used[i] = true;
        if head.matches(pool, children[i], env)
            && try_permute(pool, rest, children, used, env)
        {
            return true;
        }
        used[i] = false;
        *env = snapshot;
    }
    false
}

/// A rewrite rule: lhs pattern -> rhs builder.
pub struct Rule {
    pub name: &'static str,
    pub lhs: Pattern,
    pub rhs: Box<dyn Fn(&mut ExprPool, &MatchEnv) -> ExprId + Send + Sync>,
}

/// A set of rewrite rules.
///
/// Each registry carries a unique `id` assigned at construction via a
/// process-global monotonic counter. The id — not the registry's memory
/// address — is what `SimplifyCache` uses to partition entries, so cached
/// results computed under one registry can never leak into lookups for
/// another, even if the previous registry was dropped and its address was
/// reused by a fresh allocation.
pub struct RuleRegistry {
    pub rules: Vec<Rule>,
    id: u64,
}

/// Process-global counter for `RuleRegistry::id`. Monotonic — once issued,
/// an id is never reused, so `Drop + realloc` cannot cause id collisions.
static NEXT_REGISTRY_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl RuleRegistry {
    pub fn new() -> Self {
        let id = NEXT_REGISTRY_ID
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        RuleRegistry { rules: Vec::new(), id }
    }

    /// Stable, process-unique identity for this registry. Used as part of
    /// the `SimplifyCache` key so cached results stay correctly partitioned
    /// across distinct registries (including registries that happen to
    /// share a stack/heap address after a previous one is dropped).
    pub fn id(&self) -> u64 { self.id }

    pub fn add(&mut self, rule: Rule) { self.rules.push(rule); }

    pub fn apply(&self, pool: &mut ExprPool, expr: ExprId) -> Option<ExprId> {
        for rule in &self.rules {
            let mut env = MatchEnv::default();
            if rule.lhs.matches(pool, expr, &mut env) {
                return Some((rule.rhs)(pool, &env));
            }
        }
        None
    }
}

impl Default for RuleRegistry {
    fn default() -> Self { Self::new() }
}

// Note: RuleRegistry intentionally does NOT implement Clone — the `dyn Fn`
// inside `Rule::rhs` is not cloneable.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;
    use crate::simplify::rules::{trig_rules, DEFAULT_RULES};

    #[test]
    fn rule_registry_empty_default() {
        let reg = RuleRegistry::new();
        assert!(reg.rules.is_empty());
    }

    #[test]
    fn trig_rules_has_pythagorean() {
        let mut pool = ExprPool::new();
        let reg = trig_rules(&mut pool);
        assert!(!reg.rules.is_empty(), "trig_rules should contain at least the Pythagorean rule");
    }

    #[test]
    fn default_rules_empty() {
        // DEFAULT_RULES is a LazyLock<RuleRegistry>; deref and check.
        assert!(DEFAULT_RULES.rules.is_empty(),
                "DEFAULT_RULES must be empty (no auto trig collapse)");
    }

    #[test]
    fn rule_registry_ids_are_unique() {
        // Each construction mints a fresh id from a monotonic counter.
        let a = RuleRegistry::new();
        let b = RuleRegistry::new();
        let c = RuleRegistry::new();
        assert_ne!(a.id(), b.id());
        assert_ne!(b.id(), c.id());
        assert_ne!(a.id(), c.id());
    }

    #[test]
    fn rule_registry_id_survives_drop_and_reallocate() {
        // Regression: keying a cache by pointer address is unsound because
        // stack reuse can give two distinct registries the same `*const`.
        // The monotonic-id scheme guarantees the second id strictly
        // exceeds the first regardless of any address reuse.
        let id1 = {
            let r = RuleRegistry::new();
            r.id()
        };
        // After `r` is dropped, the next allocation may land at the same
        // address. The id must still differ.
        let id2 = RuleRegistry::new().id();
        assert!(id2 > id1, "ids must be monotonic ({} should be > {})", id2, id1);
    }

    #[test]
    fn default_rules_id_is_stable_across_lookups() {
        // The `LazyLock<RuleRegistry>` initializes once; subsequent derefs
        // must return the same id (otherwise cache reuse would be lost).
        let id_a = DEFAULT_RULES.id();
        let id_b = DEFAULT_RULES.id();
        assert_eq!(id_a, id_b);
    }
}
