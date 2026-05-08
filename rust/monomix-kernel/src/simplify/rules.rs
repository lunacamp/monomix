use crate::expr::{ExprPool, FnTag};
use crate::simplify::patterns::{MetaVar, Pattern, Rule, RuleRegistry};

/// Returns the trig rule registry containing the Pythagorean identity:
///   sin(u)^2 + cos(u)^2 -> 1
/// This is NOT part of DEFAULT_RULES — only active via simplify_trig().
pub fn trig_rules(pool: &mut ExprPool) -> RuleRegistry {
    let u_name = pool.intern_str_pub("~u");
    let u = MetaVar(u_name);
    let two = pool.small_int(2);
    let one = pool.one;

    let sin_u_sq = Pattern::Pow(
        Box::new(Pattern::Fn(FnTag::Sin, vec![Pattern::Any(u)])),
        Box::new(Pattern::Exact(two)),
    );
    let cos_u_sq = Pattern::Pow(
        Box::new(Pattern::Fn(FnTag::Cos, vec![Pattern::Any(u)])),
        Box::new(Pattern::Exact(two)),
    );

    let mut reg = RuleRegistry::new();
    reg.add(Rule {
        name: "pythagorean",
        lhs: Pattern::Add(vec![sin_u_sq, cos_u_sq]),
        rhs: Box::new(move |_pool, _env| one),
    });
    reg
}

/// DEFAULT_RULES is intentionally empty.
/// Plain `simplify()` applies NO trig identities (REDUCE-compatibility).
pub static DEFAULT_RULES: std::sync::LazyLock<RuleRegistry> =
    std::sync::LazyLock::new(RuleRegistry::new);
