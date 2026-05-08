use crate::expr::{ExprId, ExprNode, ExprPool};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

/// Coefficient: `i64` fast path with `ExprId` fallback for non-`i64` numerics
/// (BigInt, Rational), `i64`-overflow during accumulation, and arbitrary
/// symbolic sums produced by combining mixed coefficient kinds.
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
/// - `SmallInt(n)`           → (`Int(n)`, `pool.one`)
/// - `BigInt(_)` / `Rational(_)` → (`Expr(self)`, `pool.one`)  — non-i64 numerics
/// - `Neg(x)`                → (`Int(-1)`, x)
/// - `Mul([num, rest...])`   → (coeff, `pool.mul(rest)`) where `num` is the
///   first numeric child. `SmallInt` becomes `Coeff::Int`; `BigInt` /
///   `Rational` become `Coeff::Expr`. Mul children are canonically sorted by
///   `ExprId`, so the numeric coefficient may appear at any position.
/// - other → (`Int(1)`, other)
///
/// Without the `BigInt` / `Rational` arms here, terms like `(1/2)*x +
/// (1/3)*x` and `BIG*x + BIG*x` would be treated as distinct opaque bases
/// and never collected.
fn split_coeff(pool: &mut ExprPool, expr: ExprId) -> (Coeff, ExprId) {
    match pool.get(expr).clone() {
        ExprNode::SmallInt(n) => {
            let one = pool.one;
            (Coeff::Int(n), one)
        }
        ExprNode::BigInt(_) | ExprNode::Rational(_) => {
            let one = pool.one;
            (Coeff::Expr(expr), one)
        }
        ExprNode::Neg(x) => (Coeff::Int(-1), x),
        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            // Scan for the first numeric child (any of SmallInt / BigInt /
            // Rational). Mul children are sorted canonically by ExprId, so
            // a numeric coefficient may appear at any position, not just
            // index 0.
            let mut found: Option<(usize, Coeff)> = None;
            for (i, &id) in ids.iter().enumerate() {
                match pool.get(id) {
                    &ExprNode::SmallInt(n) => {
                        found = Some((i, Coeff::Int(n)));
                        break;
                    }
                    ExprNode::BigInt(_) | ExprNode::Rational(_) => {
                        found = Some((i, Coeff::Expr(id)));
                        break;
                    }
                    _ => {}
                }
            }
            if let Some((i, coeff)) = found {
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
                return (coeff, rest);
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

    #[test]
    fn collect_rational_coefficients() {
        // (1/2)*x + (1/3)*x must collect to a single term whose evaluation
        // at x=1 is 5/6 ≈ 0.833...; before this fix, the two Mul nodes were
        // treated as distinct opaque bases and never combined.
        use crate::simplify::{simplify, SimplifierConfig, SimplifyCache};
        use num_bigint::BigInt;

        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let half = pool.rational(BigInt::from(1), BigInt::from(2));
        let third = pool.rational(BigInt::from(1), BigInt::from(3));
        let half_x = pool.mul(vec![half, x]);
        let third_x = pool.mul(vec![third, x]);
        let sum = pool.add(vec![half_x, third_x]);

        let cfg = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, sum, &cfg, &mut cache);

        // Result must NOT be a 2-term Add anymore — collection should have
        // merged the like terms.
        if let ExprNode::Add(c) = pool.get(result) {
            assert_ne!(
                c.len(), 2,
                "(1/2)*x + (1/3)*x should collect into one term, got Add of len {}",
                c.len()
            );
        }
        // Numeric value at x=1 must be 5/6.
        let bindings = vec![(x, 1.0)];
        let v = crate::evalnum::evaluate_numeric(&pool, &bindings, result).unwrap();
        assert!((v - 5.0_f64 / 6.0).abs() < 1e-9, "expected 5/6, got {}", v);
    }

    #[test]
    fn collect_mixed_int_and_rational_coefficients() {
        // 2*x + (1/3)*x = 7/3 * x. At x=1, value = 7/3 ≈ 2.333...
        use crate::simplify::{simplify, SimplifierConfig, SimplifyCache};
        use num_bigint::BigInt;

        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let third = pool.rational(BigInt::from(1), BigInt::from(3));
        let two_x = pool.mul(vec![two, x]);
        let third_x = pool.mul(vec![third, x]);
        let sum = pool.add(vec![two_x, third_x]);

        let cfg = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, sum, &cfg, &mut cache);

        if let ExprNode::Add(c) = pool.get(result) {
            assert_ne!(c.len(), 2, "mixed Int+Rational should collect");
        }
        let bindings = vec![(x, 1.0)];
        let v = crate::evalnum::evaluate_numeric(&pool, &bindings, result).unwrap();
        assert!((v - 7.0_f64 / 3.0).abs() < 1e-9, "expected 7/3, got {}", v);
    }

    #[test]
    fn collect_bigint_coefficients() {
        // BIG*x + BIG*x = 2*BIG*x where BIG > i64::MAX. Verified via
        // evaluate_numeric — at x=1 the result equals 2 * BIG (as f64).
        use crate::simplify::{simplify, SimplifierConfig, SimplifyCache};
        use num_bigint::BigInt;
        use num_traits::ToPrimitive;

        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        // 2^65 — well beyond i64 range, forces BigInt representation.
        let big: BigInt = BigInt::from(1u64) << 65;
        let big_id = pool.integer(big.clone());
        // Confirm we actually got a BigInt, not a SmallInt.
        assert!(matches!(pool.get(big_id), ExprNode::BigInt(_)));
        let big_x = pool.mul(vec![big_id, x]);
        let big_x2 = pool.mul(vec![big_id, x]);
        let sum = pool.add(vec![big_x, big_x2]);

        let cfg = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, sum, &cfg, &mut cache);

        if let ExprNode::Add(c) = pool.get(result) {
            assert_ne!(
                c.len(), 2,
                "BigInt-coefficient like terms should collect"
            );
        }
        let bindings = vec![(x, 1.0)];
        let v = crate::evalnum::evaluate_numeric(&pool, &bindings, result).unwrap();
        let expected = (BigInt::from(2) * &big).to_f64().unwrap();
        assert!((v - expected).abs() / expected.abs() < 1e-12);
    }
}
