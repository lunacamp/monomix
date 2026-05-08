use crate::expr::{ExprId, ExprNode, ExprPool};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

/// Coefficient: i64 fast path with fallback to ExprId for overflow / non-integers.
#[derive(Clone, Debug)]
pub enum Coeff {
    Int(i64),
    Expr(ExprId),
}

impl Coeff {
    fn to_expr_id(&self, pool: &mut ExprPool) -> ExprId {
        match self {
            Coeff::Int(n) => pool.small_int(*n),
            Coeff::Expr(id) => *id,
        }
    }

    fn add(self, other: Coeff, pool: &mut ExprPool) -> Coeff {
        match (self, other) {
            (Coeff::Int(a), Coeff::Int(b)) => {
                if let Some(s) = a.checked_add(b) {
                    Coeff::Int(s)
                } else {
                    let ia = pool.small_int(a);
                    let ib = pool.small_int(b);
                    Coeff::Expr(pool.add(vec![ia, ib]))
                }
            }
            (a, b) => {
                let ea = a.to_expr_id(pool);
                let eb = b.to_expr_id(pool);
                Coeff::Expr(pool.add(vec![ea, eb]))
            }
        }
    }

    fn is_zero(&self, pool: &ExprPool) -> bool {
        match self {
            Coeff::Int(0) => true,
            Coeff::Expr(id) => pool.is_zero(*id),
            _ => false,
        }
    }
}

/// Extract (coefficient, base) from an expression:
/// - SmallInt(n) → (Int(n), pool.one)
/// - Mul([SmallInt(n), rest...]) → (Int(n), pool.mul(rest))
/// - Neg(x) → (Int(-1), x)
/// - other → (Int(1), other)
fn split_coeff(pool: &mut ExprPool, expr: ExprId) -> (Coeff, ExprId) {
    match pool.get(expr).clone() {
        ExprNode::SmallInt(n) => {
            let one = pool.one;
            (Coeff::Int(n), one)
        }
        ExprNode::Neg(x) => (Coeff::Int(-1), x),
        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            // Mul children are sorted canonically by ExprId, so a numeric
            // coefficient can appear at any position (in particular, not
            // necessarily at index 0). Scan for the first SmallInt.
            let mut coeff_idx: Option<usize> = None;
            let mut coeff_val: i64 = 0;
            for (i, &id) in ids.iter().enumerate() {
                if let ExprNode::SmallInt(n) = *pool.get(id) {
                    coeff_idx = Some(i);
                    coeff_val = n;
                    break;
                }
            }
            if let Some(i) = coeff_idx {
                let rest_ids: Vec<ExprId> = ids
                    .iter()
                    .enumerate()
                    .filter_map(|(j, &id)| if j == i { None } else { Some(id) })
                    .collect();
                let rest = if rest_ids.len() == 1 {
                    rest_ids[0]
                } else {
                    pool.mul(rest_ids)
                };
                return (Coeff::Int(coeff_val), rest);
            }
            (Coeff::Int(1), expr)
        }
        _ => (Coeff::Int(1), expr),
    }
}

/// Collect like terms in an Add node. Returns the input unchanged if it is
/// not an Add. Hybrid storage: SmallVec for ≤ THRESHOLD distinct bases,
/// HashMap once that limit is exceeded.
pub fn collect_like_terms(pool: &mut ExprPool, expr: ExprId) -> ExprId {
    let children = match pool.get(expr).clone() {
        ExprNode::Add(c) => c.to_vec(),
        _ => return expr,
    };

    const THRESHOLD: usize = 16;
    let mut buckets: SmallVec<[(ExprId, Coeff); 16]> = SmallVec::new();
    let mut use_map: Option<FxHashMap<ExprId, Coeff>> = None;

    for child in children {
        let (coeff, base) = split_coeff(pool, child);

        // Once upgraded, route exclusively through the map.
        if let Some(ref mut map) = use_map {
            let entry = map.entry(base).or_insert(Coeff::Int(0));
            *entry = entry.clone().add(coeff, pool);
            continue;
        }

        // Bucket-mode insert.
        if let Some(existing) = buckets.iter_mut().find(|(b, _)| *b == base) {
            existing.1 = existing.1.clone().add(coeff, pool);
            continue;
        }

        // New base. Check overflow.
        if buckets.len() >= THRESHOLD {
            let mut map: FxHashMap<ExprId, Coeff> = FxHashMap::default();
            for (b, c) in buckets.drain(..) {
                map.insert(b, c);
            }
            map.insert(base, coeff);
            use_map = Some(map);
        } else {
            buckets.push((base, coeff));
        }
    }

    let one_id = pool.one;
    let mut terms: Vec<ExprId> = Vec::new();
    let entries: Vec<(ExprId, Coeff)> = if let Some(map) = use_map {
        map.into_iter().collect()
    } else {
        buckets.into_iter().collect()
    };
    for (base, coeff) in entries {
        if coeff.is_zero(pool) { continue; }
        let c = coeff.to_expr_id(pool);
        if pool.is_one(c) {
            terms.push(base);
        } else if base == one_id {
            terms.push(c); // pure constant
        } else {
            terms.push(pool.mul(vec![c, base]));
        }
    }

    if terms.is_empty() { return pool.zero; }
    if terms.len() == 1 { return terms[0]; }
    pool.add(terms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ExprNode, ExprPool};

    #[test]
    fn collect_x_plus_x() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let sum = pool.add(vec![x, x]);
        let result = collect_like_terms(&mut pool, sum);
        // x + x = 2*x; result should contain 2 as coefficient.
        let has_two = pool.fold(result, false, &mut |found, _id, node| {
            found || matches!(node, ExprNode::SmallInt(2))
        });
        assert!(has_two, "result should contain 2 as coefficient");
    }

    #[test]
    fn collect_2x_plus_3x() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let two_x = pool.mul(vec![two, x]);
        let three_x = pool.mul(vec![three, x]);
        let sum = pool.add(vec![two_x, three_x]);
        let result = collect_like_terms(&mut pool, sum);
        let has_five = pool.fold(result, false, &mut |found, _id, node| {
            found || matches!(node, ExprNode::SmallInt(5))
        });
        assert!(has_five, "2x + 3x = 5x should contain 5");
    }

    #[test]
    fn collect_preserves_distinct_terms() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let sum = pool.add(vec![x, y]);
        let result = collect_like_terms(&mut pool, sum);
        // x + y stays as x + y
        assert!(matches!(pool.get(result), ExprNode::Add(_)));
    }
}
