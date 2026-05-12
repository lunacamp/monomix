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
/// - `Mul([factors...])`     → (coeff, base) where `coeff` is the product of
///   **every** numeric factor and `base` is the product of every remaining
///   (symbolic) factor. `SmallInt` factors stay in the i64 fast path until
///   they overflow or mix with `BigInt` / `Rational`, at which point the
///   coefficient is promoted to `Coeff::Expr`. If every factor is numeric,
///   `base` is `pool.one` (pure constant). Mul children are canonically
///   sorted by `ExprId`, so numeric and symbolic factors can interleave
///   freely — the scan is order-independent.
/// - other → (`Int(1)`, other)
///
/// Why accumulate *all* numeric factors (not just the first):
/// `pool.mul` does not fold partial numeric sub-products, so
/// `Mul([2, 3, x])` survives simplification (`fold_numeric` bails because
/// `x` is symbolic). Without this accumulation, that term would split as
/// `(coeff=2, base=Mul([3, x]))` and never collect with `Mul([6, x])`.
///
/// Why the `BigInt` / `Rational` arms exist here:
/// without them, terms like `(1/2)*x + (1/3)*x` and `BIG*x + BIG*x` would
/// be treated as distinct opaque bases and never collected.
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
            // Accumulate *every* numeric factor into a single coefficient,
            // not just the first one. Otherwise `2*3*x` splits as
            // `(coeff=2, base=3*x)` and never collects with `6*x`. We keep
            // an `i64` fast path and only promote to `Coeff::Expr` when we
            // see a non-i64 numeric or hit an `i64::checked_mul` overflow.
            let mut int_coeff: i64 = 1;
            let mut expr_factors: Vec<ExprId> = Vec::new();
            let mut base_ids: Vec<ExprId> = Vec::with_capacity(ids.len());
            for &id in &ids {
                match pool.get(id) {
                    &ExprNode::SmallInt(n) => match int_coeff.checked_mul(n) {
                        Some(prod) => int_coeff = prod,
                        None => {
                            // Overflow — spill the running i64 product into
                            // the symbolic-factor list and start over from 1
                            // so the next SmallInt has somewhere clean to go.
                            expr_factors.push(pool.small_int(int_coeff));
                            expr_factors.push(id);
                            int_coeff = 1;
                        }
                    },
                    ExprNode::BigInt(_) | ExprNode::Rational(_) => {
                        expr_factors.push(id);
                    }
                    _ => base_ids.push(id),
                }
            }

            // Combine the i64 part with the symbolic-factor list (if any).
            let coeff: Coeff = if expr_factors.is_empty() {
                Coeff::Int(int_coeff)
            } else {
                if int_coeff != 1 {
                    expr_factors.push(pool.small_int(int_coeff));
                }
                let prod = if expr_factors.len() == 1 {
                    expr_factors.pop().unwrap()
                } else {
                    pool.mul(expr_factors)
                };
                Coeff::Expr(prod)
            };

            // Rebuild the symbolic base. If every factor was numeric, the
            // Mul reduces to a pure constant — base is `1`.
            let rest = if base_ids.is_empty() {
                pool.one
            } else if base_ids.len() == 1 {
                base_ids[0]
            } else {
                pool.mul(base_ids)
            };
            return (coeff, rest);
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

    #[test]
    fn collect_multi_smallint_factors_in_mul() {
        // Mul([2, 3, x]) + Mul([6, x]) must collect to 12*x. Previously
        // split_coeff only extracted the first SmallInt, so 2*3*x split as
        // (coeff=2, base=Mul([3,x])) and never matched (coeff=6, base=x).
        use crate::simplify::{simplify, SimplifierConfig, SimplifyCache};

        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let six = pool.small_int(6);
        let two_three_x = pool.mul(vec![two, three, x]);
        let six_x = pool.mul(vec![six, x]);
        // Sanity: confirm 2*3*x really did construct as a 3-child Mul, i.e.
        // pool.mul does NOT fold partial numeric sub-products. If this ever
        // changes, the bug this test guards against won't reproduce and we
        // should reconsider the fix's scope.
        if let ExprNode::Mul(c) = pool.get(two_three_x) {
            assert_eq!(c.len(), 3, "pool.mul should keep all factors verbatim");
        } else {
            panic!("expected Mul, got {:?}", pool.get(two_three_x));
        }
        let sum = pool.add(vec![two_three_x, six_x]);

        let cfg = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, sum, &cfg, &mut cache);

        // Must collapse from two distinct Add children to a single term.
        if let ExprNode::Add(c) = pool.get(result) {
            assert_ne!(
                c.len(),
                2,
                "2*3*x + 6*x must collect; got Add of len {}",
                c.len()
            );
        }
        // Value at x=1 must be 12.
        let bindings = vec![(x, 1.0)];
        let v = crate::evalnum::evaluate_numeric(&pool, &bindings, result).unwrap();
        assert!((v - 12.0).abs() < 1e-9, "expected 12, got {}", v);
    }

    #[test]
    fn split_coeff_overflow_promotes_to_expr() {
        // i64::MAX * 2 overflows; the coefficient must promote to Coeff::Expr
        // rather than silently wrapping. Verify by checking that the value
        // remains numerically correct.
        use crate::simplify::{simplify, SimplifierConfig, SimplifyCache};

        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let max = pool.small_int(i64::MAX);
        let two = pool.small_int(2);
        // i64::MAX * 2 * x — Mul([SmallInt(MAX), SmallInt(2), x]). The i64
        // fast path overflows on `MAX * 2`; split_coeff must spill into
        // expr_factors and produce Coeff::Expr.
        let term1 = pool.mul(vec![max, two, x]);
        let term2 = pool.mul(vec![max, two, x]);
        let sum = pool.add(vec![term1, term2]);

        let cfg = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, sum, &cfg, &mut cache);

        if let ExprNode::Add(c) = pool.get(result) {
            assert_ne!(c.len(), 2, "overflow-coefficient like terms should collect");
        }
        // Sum at x=1 is 2 * (2 * i64::MAX) = 4 * i64::MAX.
        let bindings = vec![(x, 1.0)];
        let v = crate::evalnum::evaluate_numeric(&pool, &bindings, result).unwrap();
        let expected = 4.0 * (i64::MAX as f64);
        assert!((v - expected).abs() / expected.abs() < 1e-12);
    }

    #[test]
    fn split_coeff_pure_numeric_mul_has_one_base() {
        // Mul([2, 3, 4]) — no symbolic factor. The base should be `pool.one`
        // (pure constant) and the coefficient should be 24. Two such terms
        // must collect against each other AND against a bare SmallInt(24).
        use crate::simplify::{simplify, SimplifierConfig, SimplifyCache};

        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let four = pool.small_int(4);
        // Mul([2,3,4]) — pool.mul keeps all numeric children; fold_numeric
        // would collapse it on simplify, so build the sum at the like_terms
        // level by going through pool.add directly.
        let twenty_four_a = pool.mul(vec![two, three, four]);
        let twenty_four_b = pool.mul(vec![two, three, four]);
        let sum = pool.add(vec![twenty_four_a, twenty_four_b]);
        let cfg = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, sum, &cfg, &mut cache);

        // 24 + 24 = 48. Should fold cleanly to SmallInt(48).
        assert_eq!(pool.get(result), &ExprNode::SmallInt(48));
    }
}
