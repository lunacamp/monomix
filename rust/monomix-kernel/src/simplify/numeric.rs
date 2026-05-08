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
            let ids: Vec<ExprId> = children.to_vec();
            let mut p = BigInt::zero();
            let mut q = BigInt::one();
            for c in &ids {
                match pool.get(*c).clone() {
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
                    _ => return None,
                }
            }
            Some(pool.rational(p, q))
        }
        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let mut p = BigInt::one();
            let mut q = BigInt::one();
            for c in &ids {
                match pool.get(*c).clone() {
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
                (ExprNode::SmallInt(b), ExprNode::SmallInt(e)) if e >= 0 => {
                    let result = Pow::pow(BigInt::from(b), e as u32);
                    Some(pool.integer(result))
                }
                (ExprNode::SmallInt(b), ExprNode::SmallInt(e)) if e < 0 => {
                    // b^(-n) = 1/b^n
                    let bn = Pow::pow(BigInt::from(b), (-e) as u32);
                    Some(pool.rational(BigInt::one(), bn))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn negate_const(pool: &mut ExprPool, id: ExprId) -> Option<ExprId> {
    match pool.get(id).clone() {
        ExprNode::SmallInt(n) => Some(pool.small_int(-n)),
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
}
