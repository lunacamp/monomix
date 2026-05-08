use crate::expr::{ExprId, ExprNode, ExprPool};

/// Consolidate powers in a Mul node: x*x → x^2, x^a * x^b → x^(a+b).
pub fn consolidate_powers(pool: &mut ExprPool, expr: ExprId) -> ExprId {
    let children = match pool.get(expr).clone() {
        ExprNode::Mul(c) => c.to_vec(),
        _ => return expr,
    };

    use rustc_hash::FxHashMap;
    // base ExprId → accumulated exponent ExprId
    let mut exp_map: FxHashMap<ExprId, ExprId> = FxHashMap::default();
    let mut constants: Vec<ExprId> = Vec::new();
    let zero = pool.zero;

    for child in &children {
        let (base, exp) = match pool.get(*child).clone() {
            ExprNode::Pow(b, e) => (b, e),
            ExprNode::SmallInt(_) | ExprNode::Rational(_) | ExprNode::BigInt(_) => {
                constants.push(*child);
                continue;
            }
            _ => (*child, pool.one),
        };
        let entry = exp_map.entry(base).or_insert(zero);
        *entry = pool.add(vec![*entry, exp]);
    }

    let mut terms: Vec<ExprId> = constants;
    for (base, exp) in exp_map {
        // Try to fold the exponent
        if let Some(folded) = crate::simplify::numeric::fold_numeric(pool, exp) {
            terms.push(pool.pow(base, folded));
        } else {
            terms.push(pool.pow(base, exp));
        }
    }

    if terms.is_empty() { return pool.one; }
    if terms.len() == 1 { return terms[0]; }
    pool.mul(terms)
}

/// Consolidate `(x^a)^b → x^(a*b)` when a, b are integers.
pub fn consolidate_nested_pow(pool: &mut ExprPool, expr: ExprId) -> ExprId {
    if let ExprNode::Pow(base, exp) = pool.get(expr).clone() {
        if let ExprNode::Pow(inner_base, inner_exp) = pool.get(base).clone() {
            // Conservative: only when both exponents are SmallInt
            if let (ExprNode::SmallInt(a), ExprNode::SmallInt(b)) =
                (pool.get(inner_exp).clone(), pool.get(exp).clone())
            {
                if let Some(ab) = a.checked_mul(b) {
                    let new_exp = pool.small_int(ab);
                    return pool.pow(inner_base, new_exp);
                }
            }
        }
    }
    expr
}
