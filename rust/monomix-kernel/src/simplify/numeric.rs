// Task 18: simplify/numeric.rs — constant folding helper.
//
// fold_numeric attempts to reduce an Add/Mul/Pow/Neg subtree to a single
// numeric constant. Returns None if any subterm is symbolic; the caller
// then falls back to other passes (e.g., like-term collection).

use crate::expr::{ExprId, ExprNode, ExprPool};
use num_bigint::BigInt;
use num_traits::{One, Pow, Zero};

/// Attempt to fold `expr` to a single numeric constant.
/// Returns `None` if any subterm is symbolic.
pub fn fold_numeric(pool: &mut ExprPool, expr: ExprId) -> Option<ExprId> {
    match pool.get(expr).clone() {
        ExprNode::SmallInt(_)
        | ExprNode::BigInt(_)
        | ExprNode::Rational(_)
        | ExprNode::Float(_) => {
            Some(expr) // already a constant
        }
        ExprNode::Neg(x) => {
            let v = fold_numeric(pool, x)?;
            negate_const(pool, v)
        }
        ExprNode::Add(children) => {
            // Accumulate as p/q. Add integer n: p/q + n = (p + n*q) / q.
            // Add rational a/b: p/q + a/b = (p*b + a*q) / (q*b).
            //
            // Each child is recursively folded so that `Neg(<numeric>)` (the
            // parser's encoding of subtraction: `a - b` => `Add([a, Neg(b)])`)
            // — and any other child that reduces to a constant — accumulates
            // correctly. If any child fails to fold, the whole subtree is
            // symbolic and we bail.
            let ids: Vec<ExprId> = children.to_vec();
            let mut p = BigInt::zero();
            let mut q = BigInt::one();
            for c in &ids {
                let folded = fold_numeric(pool, *c)?;
                match pool.get(folded).clone() {
                    ExprNode::SmallInt(n) => {
                        p = &p + BigInt::from(n) * &q;
                    }
                    ExprNode::BigInt(big) => {
                        p = &p + &*big * &q;
                    }
                    ExprNode::Rational(b) => {
                        p = &p * &b.1 + &b.0 * &q;
                        q = &q * &b.1;
                    }
                    _ => return None, // Float — rational accumulator can't represent it
                }
            }
            Some(pool.rational(p, q))
        }
        ExprNode::Mul(children) => {
            // Same recursion as Add — handles `Neg(<numeric>)` factors and
            // any child that reduces to a rational/integer constant.
            let ids: Vec<ExprId> = children.to_vec();
            let mut p = BigInt::one();
            let mut q = BigInt::one();
            for c in &ids {
                let folded = fold_numeric(pool, *c)?;
                match pool.get(folded).clone() {
                    ExprNode::SmallInt(n) => {
                        p *= n;
                    }
                    ExprNode::BigInt(big) => {
                        p *= &*big;
                    }
                    ExprNode::Rational(b) => {
                        p *= &b.0;
                        q *= &b.1;
                    }
                    _ => return None,
                }
            }
            Some(pool.rational(p, q))
        }
        ExprNode::Pow(base, exp) => {
            match (pool.get(base).clone(), pool.get(exp).clone()) {
                (ExprNode::SmallInt(b), ExprNode::SmallInt(e)) => {
                    fold_smallint_pow(pool, b, e)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Fold `b^e` where both are `SmallInt`. `fold_numeric` must be panic-free,
/// so this routes around three pitfalls in the obvious `as u32` cast:
///
/// 1. **`i64::MIN` negation.** Computing `(-e) as u32` panics in debug and
///    wraps in release when `e == i64::MIN`. `i64::unsigned_abs` is the
///    panic-free `|e|` (yields `2^63: u64` at the boundary), and the
///    subsequent `try_into::<u32>` rejects it cleanly.
/// 2. **`as u32` truncation.** Even on the positive arm, `e as u32` silently
///    loses bits for `e > u32::MAX`. With `|b| >= 2` the result would
///    overflow any reasonable memory budget, so bailing is the right call —
///    `try_into::<u32>` does it for us.
/// 3. **`0^(-n)` division by zero.** `Pow::pow(0, n) == 0`, after which
///    `pool.rational(1, 0)` asserts. The base-specific shortcut for `b == 0`
///    short-circuits this — we return `None` rather than fold to an
///    undefined value.
///
/// Returns `None` when `|e| > u32::MAX` (with `|b| >= 2`) or when
/// `b == 0 && e < 0`. The `{-1, 0, 1}` shortcuts collapse the full `i64`
/// exponent range without ever invoking `BigInt::pow`, so those bases
/// always fold.
fn fold_smallint_pow(pool: &mut ExprPool, b: i64, e: i64) -> Option<ExprId> {
    // Base-specific shortcuts: these collapse the full i64 exponent range
    // without touching `BigInt::pow`'s u32 limit.
    if b == 0 {
        return match e.cmp(&0) {
            std::cmp::Ordering::Greater => Some(pool.small_int(0)),
            std::cmp::Ordering::Equal => Some(pool.small_int(1)), // 0^0 = 1 convention
            std::cmp::Ordering::Less => None, // 0^negative is undefined; don't fold
        };
    }
    if b == 1 { return Some(pool.small_int(1)); }
    if b == -1 {
        // (-1)^e: 1 if e is even, -1 if odd. `i64::MIN % 2 == 0` so this
        // is well-defined across the full i64 range.
        return Some(pool.small_int(if e % 2 == 0 { 1 } else { -1 }));
    }

    // |b| >= 2: bail if |e| can't fit in `u32` (BigInt::pow's argument type).
    // `unsigned_abs` is the panic-free `|e|`; the `try_into` is the actual
    // safety check.
    let exp_u32: u32 = e.unsigned_abs().try_into().ok()?;
    let pow = Pow::pow(BigInt::from(b), exp_u32);
    if e >= 0 {
        Some(pool.integer(pow))
    } else {
        Some(pool.rational(BigInt::one(), pow))
    }
}

fn negate_const(pool: &mut ExprPool, id: ExprId) -> Option<ExprId> {
    match pool.get(id).clone() {
        ExprNode::SmallInt(n) => match n.checked_neg() {
            // `-i64::MIN` overflows `i64` (`checked_neg` returns `None`).
            // Promote through `BigInt` so `fold_numeric` stays panic-free
            // in debug and avoids silent wrap in release. `pool.integer`
            // canonicalises back to `SmallInt` when the result fits, so
            // the only observable change is at the `i64::MIN` boundary.
            Some(neg) => Some(pool.small_int(neg)),
            None => Some(pool.integer(-BigInt::from(n))),
        },
        ExprNode::BigInt(b) => Some(pool.integer(-(*b))),
        ExprNode::Rational(b) => Some(pool.rational(-b.0, b.1)),
        ExprNode::Float(f) => Some(pool.float(-f.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;

    #[test]
    fn fold_integer_add() {
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let sum = pool.add(vec![two, three]);
        let result = fold_numeric(&mut pool, sum).unwrap();
        assert_eq!(pool.get(result), &ExprNode::SmallInt(5));
    }

    #[test]
    fn fold_integer_mul() {
        let mut pool = ExprPool::new();
        let four = pool.small_int(4);
        let five = pool.small_int(5);
        let prod = pool.mul(vec![four, five]);
        let result = fold_numeric(&mut pool, prod).unwrap();
        assert_eq!(pool.get(result), &ExprNode::SmallInt(20));
    }

    #[test]
    fn fold_rational_add() {
        let mut pool = ExprPool::new();
        use num_bigint::BigInt;
        let half = pool.rational(BigInt::from(1), BigInt::from(2));
        let third = pool.rational(BigInt::from(1), BigInt::from(3));
        let sum = pool.add(vec![half, third]);
        let result = fold_numeric(&mut pool, sum).unwrap();
        // 1/2 + 1/3 = 5/6
        if let ExprNode::Rational(b) = pool.get(result) {
            assert_eq!(b.0, BigInt::from(5));
            assert_eq!(b.1, BigInt::from(6));
        } else {
            panic!("expected Rational(5,6), got {:?}", pool.get(result));
        }
    }

    #[test]
    fn fold_mixed_numeric_symbolic_returns_none() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let sum = pool.add(vec![x, two]);
        let result = fold_numeric(&mut pool, sum);
        assert!(result.is_none(), "mixed numeric+symbolic should not fold");
    }

    #[test]
    fn fold_pow_two_to_the_three() {
        // 2^3 = 8 — Pow arm folds SmallInt^SmallInt with e >= 0.
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let expr = pool.pow(two, three);
        let result = fold_numeric(&mut pool, expr).unwrap();
        assert_eq!(pool.get(result), &ExprNode::SmallInt(8));
    }

    #[test]
    fn fold_pow_with_neg_base_returns_none() {
        // Pow with non-numeric (Neg-wrapped) base is not foldable in Phase 1.
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let neg_two = pool.neg(two);
        let three = pool.small_int(3);
        let expr = pool.pow(neg_two, three);
        let result = fold_numeric(&mut pool, expr);
        assert!(result.is_none(), "Pow(Neg(_), _) is not foldable yet");
    }

    #[test]
    fn fold_subtraction_via_neg_in_add() {
        // Regression: parser lowers `10 - 3` to `Add([10, Neg(3)])`.
        // Previously this returned None because Neg wasn't a recognized
        // numeric atom inside the Add accumulation.
        let mut pool = ExprPool::new();
        let ten = pool.small_int(10);
        let three = pool.small_int(3);
        let neg_three = pool.neg(three);
        let diff = pool.add(vec![ten, neg_three]);
        let result = fold_numeric(&mut pool, diff).unwrap();
        assert_eq!(pool.get(result), &ExprNode::SmallInt(7));
    }

    #[test]
    fn fold_neg_factor_in_mul() {
        // Regression: `2 * (-3)` is `Mul([2, Neg(3)])`. Previously unfoldable.
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let neg_three = pool.neg(three);
        let prod = pool.mul(vec![two, neg_three]);
        let result = fold_numeric(&mut pool, prod).unwrap();
        assert_eq!(pool.get(result), &ExprNode::SmallInt(-6));
    }

    #[test]
    fn fold_subtraction_with_rationals() {
        // 1/2 + Neg(1/3) = 1/6
        let mut pool = ExprPool::new();
        use num_bigint::BigInt;
        let half = pool.rational(BigInt::from(1), BigInt::from(2));
        let third = pool.rational(BigInt::from(1), BigInt::from(3));
        let neg_third = pool.neg(third);
        let diff = pool.add(vec![half, neg_third]);
        let result = fold_numeric(&mut pool, diff).unwrap();
        if let ExprNode::Rational(b) = pool.get(result) {
            assert_eq!(b.0, BigInt::from(1));
            assert_eq!(b.1, BigInt::from(6));
        } else {
            panic!("expected Rational(1, 6), got {:?}", pool.get(result));
        }
    }

    #[test]
    fn fold_pow_i64_min_exponent_with_huge_base_bails() {
        // Regression: previously `(-e) as u32` with `e == i64::MIN` either
        // panicked (debug) or wrapped to 0 (release), making `b^i64::MIN`
        // fold to `1/1 = 1` for any `b` — wildly wrong. `|b| >= 2` with
        // `|e| > u32::MAX` must return `None` so the caller falls through.
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let min = pool.small_int(i64::MIN);
        let expr = pool.pow(two, min);
        let result = fold_numeric(&mut pool, expr);
        assert!(result.is_none(), "2^i64::MIN must bail, not fold (got {:?})",
                result.map(|id| pool.get(id).clone()));
    }

    #[test]
    fn fold_pow_huge_positive_exponent_bails() {
        // Symmetric to the i64::MIN case: `e > u32::MAX` would previously
        // silently truncate via `e as u32`, producing a wrong result. With
        // |b| >= 2 we now bail.
        let mut pool = ExprPool::new();
        let three = pool.small_int(3);
        let huge = pool.small_int(u32::MAX as i64 + 1);
        let expr = pool.pow(three, huge);
        let result = fold_numeric(&mut pool, expr);
        assert!(result.is_none(),
                "3^(u32::MAX+1) must bail (would truncate to 3^0=1 under \
                 the old `as u32` cast); got {:?}",
                result.map(|id| pool.get(id).clone()));
    }

    #[test]
    fn fold_pow_minus_one_to_i64_min_is_one() {
        // (-1)^e is exactly representable for any i64 exponent. `i64::MIN`
        // is even, so the result is 1 — must fold, not bail.
        let mut pool = ExprPool::new();
        let neg_one = pool.small_int(-1);
        let min = pool.small_int(i64::MIN);
        let expr = pool.pow(neg_one, min);
        let result = fold_numeric(&mut pool, expr).unwrap();
        assert_eq!(pool.get(result), &ExprNode::SmallInt(1));
    }

    #[test]
    fn fold_pow_minus_one_to_huge_odd_is_minus_one() {
        // (-1)^(odd) = -1 across the full i64 range; `i64::MAX` is odd.
        let mut pool = ExprPool::new();
        let neg_one = pool.small_int(-1);
        let max = pool.small_int(i64::MAX);
        let expr = pool.pow(neg_one, max);
        let result = fold_numeric(&mut pool, expr).unwrap();
        assert_eq!(pool.get(result), &ExprNode::SmallInt(-1));
    }

    #[test]
    fn fold_pow_one_to_i64_min_is_one() {
        // 1^anything = 1 — must fold regardless of how pathological the
        // exponent is.
        let mut pool = ExprPool::new();
        let one = pool.small_int(1);
        let min = pool.small_int(i64::MIN);
        let expr = pool.pow(one, min);
        let result = fold_numeric(&mut pool, expr).unwrap();
        assert_eq!(pool.get(result), &ExprNode::SmallInt(1));
    }

    #[test]
    fn fold_pow_zero_to_negative_does_not_panic() {
        // Pre-fix: `Pow::pow(0, n) == 0`, then `pool.rational(1, 0)`
        // panicked with "rational: denominator is zero". Must now return
        // `None` (0^negative is mathematically undefined, not foldable).
        //
        // Note: `pool.pow(zero, neg)` does *not* short-circuit (only
        // `pool.pow(_, 0)` and `pool.pow(_, 1)` do), so this path is
        // reachable through normal construction.
        let mut pool = ExprPool::new();
        let zero = pool.small_int(0);
        let neg_three = pool.small_int(-3);
        let expr = pool.pow(zero, neg_three);
        let result = fold_numeric(&mut pool, expr);
        assert!(result.is_none(), "0^(-3) must not fold (got {:?})",
                result.map(|id| pool.get(id).clone()));
    }

    #[test]
    fn fold_pow_negative_exponent_still_folds_normally() {
        // Make sure the rewrite didn't break the existing happy path:
        // `2^(-3) = 1/8`.
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let neg_three = pool.small_int(-3);
        let expr = pool.pow(two, neg_three);
        let result = fold_numeric(&mut pool, expr).unwrap();
        use num_bigint::BigInt;
        if let ExprNode::Rational(b) = pool.get(result) {
            assert_eq!(b.0, BigInt::from(1));
            assert_eq!(b.1, BigInt::from(8));
        } else {
            panic!("expected Rational(1, 8), got {:?}", pool.get(result));
        }
    }

    #[test]
    fn fold_neg_of_i64_min_does_not_panic() {
        // Regression: `negate_const` previously did `pool.small_int(-n)` for
        // `SmallInt(i64::MIN)`. The negation overflows i64 — panics in debug,
        // wraps to i64::MIN in release. Both are wrong: `fold_numeric` must
        // be panic-free, and the wrapping result loses sign information.
        // The fix promotes through BigInt; the canonicaliser in
        // `pool.integer` re-narrows to SmallInt when the result fits, so
        // only the `i64::MIN` boundary actually allocates a BigInt.
        let mut pool = ExprPool::new();
        let min = pool.small_int(i64::MIN);
        let neg = pool.neg(min);
        let result = fold_numeric(&mut pool, neg).unwrap();
        // `-i64::MIN` is `i64::MAX + 1`, which doesn't fit in i64.
        let expected_big = -BigInt::from(i64::MIN);
        match pool.get(result) {
            ExprNode::BigInt(b) => assert_eq!(&**b, &expected_big),
            other => panic!("expected BigInt({}), got {:?}", expected_big, other),
        }
    }

    #[test]
    fn fold_neg_of_i64_min_inside_add() {
        // Same as above but exercises the path through `fold_numeric`'s Add
        // arm, since that's the realistic call site (parser lowers `a - b`
        // to `Add([a, Neg(b)])`). Result is a Rational(p, 1) where p is the
        // BigInt-promoted value; downstream consumers should see no panic.
        let mut pool = ExprPool::new();
        let min = pool.small_int(i64::MIN);
        let one = pool.small_int(1);
        let neg_min = pool.neg(min);
        let sum = pool.add(vec![one, neg_min]);
        let result = fold_numeric(&mut pool, sum).unwrap();
        // 1 + (-i64::MIN) = 1 + (2^63) = 2^63 + 1
        let expected = BigInt::from(1) + (-BigInt::from(i64::MIN));
        match pool.get(result) {
            ExprNode::BigInt(b) => assert_eq!(&**b, &expected),
            other => panic!("expected BigInt({}), got {:?}", expected, other),
        }
    }

    #[test]
    fn fold_neg_of_symbolic_still_returns_none() {
        // `Neg(x)` inside Add must not pretend to be numeric.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let neg_x = pool.neg(x);
        let one = pool.small_int(1);
        let sum = pool.add(vec![one, neg_x]);
        let result = fold_numeric(&mut pool, sum);
        assert!(result.is_none(), "Add([1, Neg(x)]) must not fold");
    }
}
