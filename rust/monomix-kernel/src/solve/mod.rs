use crate::error::KernelError;
use crate::expr::{ExprId, ExprNode, ExprPool};
use crate::poly::{coeff, deg};
use crate::simplify::{simplify, SimplifierConfig, SimplifyCache};

pub type Substitution = Vec<(ExprId, ExprId)>;

/// The result of solving an equation (or system) for one or more variables.
///
/// `solutions` contains one `Substitution` per **distinct** solution.
/// Multiplicity is not encoded — a quadratic with a double root returns
/// exactly one substitution, matching the convention used by Mathematica's
/// `Solve` and SymPy's `solve`. A truly empty `solutions` means the equation
/// has no solutions in the represented domain (e.g. real-only quadratic with
/// negative discriminant — see `has_complex_roots`).
///
/// A single empty substitution (`vec![vec![]]`) is the conventional
/// encoding for a tautology (e.g. `0 = 0`) — every value of the variable
/// satisfies the equation.
#[derive(Debug)]
pub struct SolutionSet {
    pub solutions: Vec<Substitution>,
    pub has_complex_roots: bool,
}

/// Solve `eq` for `var`. `eq` may be an `Eq(lhs, rhs)` node or a bare
/// expression treated as `expr = 0`.
pub fn solve(
    pool: &mut ExprPool,
    eq: ExprId,
    var: ExprId,
) -> Result<SolutionSet, KernelError> {
    let (lhs, rhs) = match pool.get(eq) {
        ExprNode::Eq(l, r) => (*l, *r),
        _ => {
            let z = pool.zero;
            (eq, z)
        }
    };
    // Move everything to lhs: lhs - rhs = 0
    let rhs_neg = pool.neg(rhs);
    let poly_expr = pool.add(vec![lhs, rhs_neg]);

    let degree = deg(pool, poly_expr, var);
    match degree {
        None => Err(KernelError::UnsupportedEquation {
            reason: "expression is not polynomial in the given variable".to_string(),
        }),
        Some(0) => {
            // The polynomial is constant in `var`. We must distinguish:
            //   - tautology  (0 = 0)        — every value of `var` satisfies it
            //   - contradiction (c = 0, c != 0) — no value of `var` satisfies it
            //
            // Encoding: a single empty substitution stands for "any value of
            // `var` works" (tautology); an empty solutions list stands for
            // "no value works" (contradiction). This mirrors Mathematica's
            // `Solve[{0 == 0}, x]` returning `{{}}` and `Solve[{1 == 0}, x]`
            // returning `{}`.
            let config = SimplifierConfig::default();
            let mut cache = SimplifyCache::new();
            let constant = simplify(pool, poly_expr, &config, &mut cache);
            if pool.is_zero(constant) {
                Ok(SolutionSet { solutions: vec![vec![]], has_complex_roots: false })
            } else {
                Ok(SolutionSet { solutions: vec![], has_complex_roots: false })
            }
        }
        Some(1) => solve_linear(pool, poly_expr, var),
        Some(2) => solve_quadratic(pool, poly_expr, var),
        Some(d) => Err(KernelError::UnsupportedEquation {
            reason: format!("degree {} polynomial (only linear and quadratic supported)", d),
        }),
    }
}

fn solve_linear(
    pool: &mut ExprPool,
    poly_expr: ExprId,
    var: ExprId,
) -> Result<SolutionSet, KernelError> {
    // a*x + b = 0  =>  x = -b/a
    let a = coeff(pool, poly_expr, var, 1);
    let b = coeff(pool, poly_expr, var, 0);
    if pool.is_zero(a) {
        return Err(KernelError::UnsupportedEquation {
            reason: "coefficient of variable is zero in linear solve".to_string(),
        });
    }
    let neg_b = pool.neg(b);
    let val = pool.div(neg_b, a);
    let config = SimplifierConfig::default();
    let mut cache = SimplifyCache::new();
    let simplified_val = simplify(pool, val, &config, &mut cache);
    Ok(SolutionSet {
        solutions: vec![vec![(var, simplified_val)]],
        has_complex_roots: false,
    })
}

fn solve_quadratic(
    pool: &mut ExprPool,
    poly_expr: ExprId,
    var: ExprId,
) -> Result<SolutionSet, KernelError> {
    // a*x^2 + b*x + c = 0
    let config = SimplifierConfig::default();
    let mut cache = SimplifyCache::new();
    let a_raw = coeff(pool, poly_expr, var, 2);
    let b_raw = coeff(pool, poly_expr, var, 1);
    let c_raw = coeff(pool, poly_expr, var, 0);
    let a = simplify(pool, a_raw, &config, &mut cache);
    let b = simplify(pool, b_raw, &config, &mut cache);
    let c = simplify(pool, c_raw, &config, &mut cache);

    // discriminant = b^2 - 4*a*c
    // Short-circuit b^2 when b is zero (simplifier does not fold 0^n).
    let b2 = if pool.is_zero(b) {
        pool.zero
    } else {
        let two_int = pool.small_int(2);
        pool.pow(b, two_int)
    };
    let four = pool.small_int(4);
    let four_ac = pool.mul(vec![four, a, c]);
    let neg_four_ac = pool.neg(four_ac);
    let discriminant = pool.add(vec![b2, neg_four_ac]);
    let disc_simplified = simplify(pool, discriminant, &config, &mut cache);

    // Classify the discriminant exactly when possible. Going through `f64`
    // misclassifies tiny rationals (underflow → false `Zero`) and huge
    // BigInts/Rationals (overflow → NaN/Inf comparison surprises). Falls back
    // to `f64` only for `Float` — for which we have no exact representation
    // and the user has opted into floating-point semantics.
    match classify_discriminant(pool, disc_simplified) {
        Some(DiscSign::Negative) => {
            return Ok(SolutionSet { solutions: vec![], has_complex_roots: true });
        }
        Some(DiscSign::Zero) => {
            // Double root: x = -b / (2a). Returned once — `SolutionSet`
            // does not encode multiplicity, matching the convention used
            // by Mathematica's `Solve` and SymPy's `solve`. Callers that
            // need to know about multiplicity should inspect the
            // discriminant directly.
            let two_int2 = pool.small_int(2);
            let two_a = pool.mul(vec![two_int2, a]);
            let neg_b_local = pool.neg(b);
            let val = pool.div(neg_b_local, two_a);
            let s = simplify(pool, val, &config, &mut cache);
            return Ok(SolutionSet {
                solutions: vec![vec![(var, s)]],
                has_complex_roots: false,
            });
        }
        Some(DiscSign::Positive) | None => {
            // Positive → fall through to the two-roots path below.
            // None → discriminant is symbolic (not a numeric atom); we can't
            // decide the sign at this point. Fall through to the symbolic
            // two-roots path with `sqrt(disc)` left unevaluated.
        }
    }

    // Two roots: x = (-b +/- sqrt(disc)) / (2a)
    let sqrt_disc = pool.func(crate::expr::FnTag::Sqrt, vec![disc_simplified]);
    let two_int3 = pool.small_int(2);
    let two_a = pool.mul(vec![two_int3, a]);
    let neg_b = pool.neg(b);

    let root1_num = pool.add(vec![neg_b, sqrt_disc]);
    let root1 = pool.div(root1_num, two_a);
    let root1 = simplify(pool, root1, &config, &mut cache);

    let neg_b2 = pool.neg(b);
    let neg_sqrt_disc = pool.neg(sqrt_disc);
    let root2_num = pool.add(vec![neg_b2, neg_sqrt_disc]);
    let root2 = pool.div(root2_num, two_a);
    let root2 = simplify(pool, root2, &config, &mut cache);

    Ok(SolutionSet {
        solutions: vec![vec![(var, root1)], vec![(var, root2)]],
        has_complex_roots: false,
    })
}

fn try_to_f64(pool: &ExprPool, expr: ExprId) -> Option<f64> {
    use num_traits::ToPrimitive;
    match pool.get(expr) {
        ExprNode::SmallInt(n) => Some(*n as f64),
        ExprNode::BigInt(n) => n.to_f64(),
        ExprNode::Rational(b) => {
            let p = b.0.to_f64()?;
            let q = b.1.to_f64()?;
            Some(p / q)
        }
        ExprNode::Float(f) => Some(f.0),
        ExprNode::Neg(inner) => try_to_f64(pool, *inner).map(|v| -v),
        _ => None,
    }
}

/// Sign of a discriminant: used by the quadratic solver to pick between the
/// complex-roots / double-root / two-real-roots branches without going through
/// `f64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscSign {
    Negative,
    Zero,
    Positive,
}

/// Classify the sign of `expr` exactly when it's a numeric atom (or a `Neg`
/// of one); fall back to `f64` only for `Float`, which has no exact form to
/// inspect. Returns `None` when the expression is symbolic — callers must
/// handle that case (typically by emitting a symbolic `sqrt(disc)`).
///
/// The pool's `rational` constructor normalizes denominators to be positive
/// and rejects zero numerators (a zero rational becomes the pre-interned
/// `SmallInt(0)`), so a live `Rational` node is always nonzero and its sign
/// is exactly its numerator's sign.
fn classify_discriminant(pool: &ExprPool, expr: ExprId) -> Option<DiscSign> {
    use num_bigint::Sign;
    match pool.get(expr) {
        ExprNode::SmallInt(n) => Some(match n.cmp(&0) {
            std::cmp::Ordering::Less => DiscSign::Negative,
            std::cmp::Ordering::Equal => DiscSign::Zero,
            std::cmp::Ordering::Greater => DiscSign::Positive,
        }),
        ExprNode::BigInt(b) => Some(match b.sign() {
            Sign::Minus => DiscSign::Negative,
            Sign::NoSign => DiscSign::Zero,
            Sign::Plus => DiscSign::Positive,
        }),
        ExprNode::Rational(b) => Some(match b.0.sign() {
            Sign::Minus => DiscSign::Negative,
            // The pool normalizes zero rationals to `SmallInt(0)`, so this
            // arm is defensive and should never fire under normal use.
            Sign::NoSign => DiscSign::Zero,
            Sign::Plus => DiscSign::Positive,
        }),
        ExprNode::Neg(inner) => classify_discriminant(pool, *inner).map(|s| match s {
            DiscSign::Negative => DiscSign::Positive,
            DiscSign::Zero => DiscSign::Zero,
            DiscSign::Positive => DiscSign::Negative,
        }),
        ExprNode::Float(f) => {
            let v = f.0;
            if v.is_nan() {
                None
            } else if v < 0.0 {
                Some(DiscSign::Negative)
            } else if v > 0.0 {
                Some(DiscSign::Positive)
            } else {
                // covers +0.0 and -0.0
                Some(DiscSign::Zero)
            }
        }
        _ => None,
    }
}

/// Solve a linear n x n system of equations (numeric coefficients only) via
/// Gaussian elimination with partial pivoting.
///
/// Each equation must be `Eq(lhs, rhs)` (or a bare expression treated as
/// `expr = 0`). Coefficients are extracted via numeric evaluation:
///   - constant term `b` = E(all vars = 0)
///   - coefficient `a_j` = E(x_j=1, others=0) - b
///
/// **Linearity is verified by additional probing**: for each variable `j`
/// the equation is also evaluated at `x_j=2` (others=0) and required to
/// match `2·a_j + b`; for each pair (i, j) the equation is evaluated at
/// `x_i=1, x_j=1` (others=0) and required to match `a_i + a_j + b`. Any
/// mismatch (within a small tolerance) means the equation is nonlinear or
/// has a cross-term, and `UnsupportedEquation` is returned. Without this
/// check, a probe at 0/1 alone would silently accept e.g. `x²+y=0` or
/// `x·y=0` and produce wrong "solutions".
///
/// Phase 1 limitation: coefficients must be numeric (BigInt / Rational /
/// Float). Symbolic coefficients return `UnsupportedEquation`. Multivariate
/// polynomial extraction is deferred to Phase 2.
pub fn solve_system(
    pool: &mut ExprPool,
    equations: &[ExprId],
    vars: &[ExprId],
) -> Result<SolutionSet, KernelError> {
    use crate::evalnum::evaluate_numeric;

    let n = vars.len();
    if equations.len() != n {
        return Err(KernelError::UnsupportedEquation {
            reason: "number of equations must equal number of unknowns".to_string(),
        });
    }

    let zero_bindings: Vec<(ExprId, f64)> =
        vars.iter().map(|&v| (v, 0.0)).collect();

    // Closure to evaluate the equation at a particular variable assignment,
    // mapping any numeric-eval failure to UnsupportedEquation.
    let eval_at = |pool: &ExprPool, expr: ExprId, bindings: &[(ExprId, f64)]|
        -> Result<f64, KernelError> {
        evaluate_numeric(pool, bindings, expr).map_err(|_| {
            KernelError::UnsupportedEquation {
                reason: "non-numeric coefficient in linear system".to_string(),
            }
        })
    };

    // Closeness check used by the linearity verification. Same shape as
    // numpy.isclose: |a - b| <= atol + rtol·max(|a|, |b|). Both atol and rtol
    // are 1e-9 — tight enough to catch a missing/extra integer coefficient,
    // loose enough that float-arithmetic round-off in the eval doesn't
    // false-positive.
    let close = |actual: f64, expected: f64| -> bool {
        let mag = actual.abs().max(expected.abs());
        (actual - expected).abs() <= 1e-9 + 1e-9 * mag
    };

    let mut mat: Vec<Vec<f64>> = Vec::with_capacity(n);
    for (eq_idx, &eq) in equations.iter().enumerate() {
        let (lhs, rhs) = match pool.get(eq) {
            ExprNode::Eq(l, r) => (*l, *r),
            _ => {
                let z = pool.zero;
                (eq, z)
            }
        };
        let rhs_neg = pool.neg(rhs);
        let poly_expr = pool.add(vec![lhs, rhs_neg]);

        let const_val = eval_at(pool, poly_expr, &zero_bindings)?;

        let mut row = Vec::with_capacity(n + 1);
        for j in 0..n {
            let mut bj = zero_bindings.clone();
            bj[j].1 = 1.0;
            let ej = eval_at(pool, poly_expr, &bj)?;
            row.push(ej - const_val);
        }

        // Verification probe 1: each variable in isolation must be linear.
        // Evaluate at x_j = 2 (others = 0) and check against 2·a_j + b.
        // This catches purely-univariate nonlinearities like x², x³, sin(x).
        for j in 0..n {
            let mut bj = zero_bindings.clone();
            bj[j].1 = 2.0;
            let probed = eval_at(pool, poly_expr, &bj)?;
            let predicted = 2.0 * row[j] + const_val;
            if !close(probed, predicted) {
                return Err(KernelError::UnsupportedEquation {
                    reason: format!(
                        "equation {} is nonlinear in variable index {} \
                         (probe at 2 yielded {}, linear model predicted {})",
                        eq_idx, j, probed, predicted
                    ),
                });
            }
        }

        // Verification probe 2: no cross-terms between any variable pair.
        // Evaluate at x_i=1, x_k=1 (others=0) and check against a_i + a_k + b.
        // This catches `x·y`, `x·(y+1)`, etc., which probe-1 misses because
        // a single variable being 1 with the other 0 makes the product vanish.
        for i in 0..n {
            for k in (i + 1)..n {
                let mut b_ik = zero_bindings.clone();
                b_ik[i].1 = 1.0;
                b_ik[k].1 = 1.0;
                let probed = eval_at(pool, poly_expr, &b_ik)?;
                let predicted = row[i] + row[k] + const_val;
                if !close(probed, predicted) {
                    return Err(KernelError::UnsupportedEquation {
                        reason: format!(
                            "equation {} contains a cross-term between \
                             variable indices {} and {} \
                             (probe at (1,1) yielded {}, linear model predicted {})",
                            eq_idx, i, k, probed, predicted
                        ),
                    });
                }
            }
        }

        row.push(-const_val);
        mat.push(row);
    }

    // Gaussian elimination with partial pivoting.
    for col in 0..n {
        let mut pivot_row = col;
        let mut best = mat[col][col].abs();
        for r in (col + 1)..n {
            if mat[r][col].abs() > best {
                pivot_row = r;
                best = mat[r][col].abs();
            }
        }
        mat.swap(col, pivot_row);
        let pivot = mat[col][col];
        if pivot.abs() < 1e-12 {
            return Err(KernelError::SingularSystem);
        }
        for row in (col + 1)..n {
            let factor = mat[row][col] / pivot;
            for k in col..=n {
                let v = mat[col][k];
                mat[row][k] -= factor * v;
            }
        }
    }

    // Back substitution.
    let mut solution = vec![0.0f64; n];
    for i in (0..n).rev() {
        let mut s = mat[i][n];
        for j in (i + 1)..n {
            s -= mat[i][j] * solution[j];
        }
        solution[i] = s / mat[i][i];
    }

    let binding: Substitution = vars
        .iter()
        .zip(solution.iter())
        .map(|(&var, &val)| (var, pool.float(val)))
        .collect();
    Ok(SolutionSet {
        solutions: vec![binding],
        has_complex_roots: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;
    use crate::error::KernelError;

    #[test]
    fn solve_linear_x_minus_3() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let three = pool.small_int(3);
        let zero = pool.zero;
        // x - 3 = 0
        let neg3 = pool.neg(three);
        let expr = pool.add(vec![x, neg3]);
        let eq = pool.eq_node(expr, zero);
        let result = solve(&mut pool, eq, x).unwrap();
        assert!(!result.has_complex_roots);
        assert_eq!(result.solutions.len(), 1);
        let binding = &result.solutions[0];
        assert_eq!(binding.len(), 1);
        assert_eq!(binding[0].0, x);
        assert_eq!(binding[0].1, three);
    }

    #[test]
    fn solve_quadratic_x_squared_minus_4() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let zero = pool.zero;
        // x^2 - 4 = 0 -> x = +/-2
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let four = pool.small_int(4);
        let neg4 = pool.neg(four);
        let poly = pool.add(vec![x2, neg4]);
        let eq = pool.eq_node(poly, zero);
        let result = solve(&mut pool, eq, x).unwrap();
        assert!(!result.has_complex_roots);
        assert_eq!(result.solutions.len(), 2);
    }

    #[test]
    fn solve_quadratic_complex_roots() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        let zero = pool.zero;
        // x^2 + 1 = 0 -> complex roots
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let poly = pool.add(vec![x2, one]);
        let eq = pool.eq_node(poly, zero);
        let result = solve(&mut pool, eq, x).unwrap();
        assert!(result.has_complex_roots);
        assert!(result.solutions.is_empty());
    }

    #[test]
    fn solve_unsupported_cubic_errors() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        // x^3 - 1 = 0 -> UnsupportedEquation
        let three_int = pool.small_int(3);
        let x3 = pool.pow(x, three_int);
        let eq = pool.eq_node(x3, one);
        let result = solve(&mut pool, eq, x);
        assert!(matches!(result, Err(KernelError::UnsupportedEquation { .. })));
    }

    #[test]
    fn solve_tautology_zero_equals_zero() {
        // 0 = 0: every x is a solution. Encoded as one empty substitution.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let zero = pool.zero;
        let eq = pool.eq_node(zero, zero);
        let result = solve(&mut pool, eq, x).unwrap();
        assert!(!result.has_complex_roots);
        assert_eq!(
            result.solutions.len(),
            1,
            "tautology should produce exactly one (empty) substitution"
        );
        assert!(
            result.solutions[0].is_empty(),
            "tautology's substitution must not bind `var`; got {:?}",
            result.solutions[0]
        );
    }

    #[test]
    fn solve_tautology_after_simplify() {
        // 5 = 5: rearranged to `5 - 5 = 0`, simplifies to 0, so it's a tautology.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let five = pool.small_int(5);
        let eq = pool.eq_node(five, five);
        let result = solve(&mut pool, eq, x).unwrap();
        assert_eq!(result.solutions.len(), 1);
        assert!(result.solutions[0].is_empty());
    }

    #[test]
    fn solve_contradiction_nonzero_constant() {
        // 5 = 0: no x satisfies it.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let zero = pool.zero;
        let five = pool.small_int(5);
        let eq = pool.eq_node(five, zero);
        let result = solve(&mut pool, eq, x).unwrap();
        assert!(!result.has_complex_roots);
        assert!(
            result.solutions.is_empty(),
            "contradiction must have no solutions"
        );
    }

    #[test]
    fn solve_quadratic_double_root_returns_single_solution() {
        // x^2 - 4x + 4 = (x - 2)^2 = 0 has the double root x = 2.
        // SolutionSet does not encode multiplicity, so we expect exactly
        // one substitution (regression: previously returned two identical
        // copies, misleading downstream code that expected unique roots).
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let zero = pool.zero;
        let two_int = pool.small_int(2);
        let four = pool.small_int(4);
        let neg_four = pool.small_int(-4);
        let x2 = pool.pow(x, two_int);
        let neg_four_x = pool.mul(vec![neg_four, x]);
        let poly = pool.add(vec![x2, neg_four_x, four]);
        let eq = pool.eq_node(poly, zero);
        let result = solve(&mut pool, eq, x).unwrap();
        assert!(!result.has_complex_roots);
        assert_eq!(
            result.solutions.len(),
            1,
            "double root must return one substitution, not duplicates"
        );
        let binding = &result.solutions[0];
        assert_eq!(binding.len(), 1);
        assert_eq!(binding[0].0, x);
        // Value check via numeric eval — robust to whichever symbolic form
        // the simplifier leaves behind for `-(-4) / (2 * 1)`.
        let val = crate::evalnum::evaluate_numeric(&pool, &[], binding[0].1)
            .expect("double-root value should be numerically evaluable");
        assert_eq!(val, 2.0);
    }

    #[test]
    fn solve_contradiction_after_simplify() {
        // 7 = 4: rearranged to `7 - 4 = 0`, simplifies to 3 (nonzero) → no solutions.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let seven = pool.small_int(7);
        let four = pool.small_int(4);
        let eq = pool.eq_node(seven, four);
        let result = solve(&mut pool, eq, x).unwrap();
        assert!(result.solutions.is_empty());
    }

    // ---- solve_system ---------------------------------------------------

    #[test]
    fn solve_system_2x2_linear() {
        // 2x + y = 5
        //  x - y = 1   →  x = 2, y = 1
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let two = pool.small_int(2);
        let neg_one = pool.small_int(-1);
        let five = pool.small_int(5);
        let one = pool.one;

        let two_x = pool.mul(vec![two, x]);
        let lhs1 = pool.add(vec![two_x, y]);
        let eq1 = pool.eq_node(lhs1, five);

        let neg_y = pool.mul(vec![neg_one, y]);
        let lhs2 = pool.add(vec![x, neg_y]);
        let eq2 = pool.eq_node(lhs2, one);

        let result = solve_system(&mut pool, &[eq1, eq2], &[x, y]).unwrap();
        assert_eq!(result.solutions.len(), 1);
        let binding = &result.solutions[0];
        assert_eq!(binding.len(), 2);
        let x_val = crate::evalnum::evaluate_numeric(&pool, &[], binding[0].1).unwrap();
        let y_val = crate::evalnum::evaluate_numeric(&pool, &[], binding[1].1).unwrap();
        assert!((x_val - 2.0).abs() < 1e-9);
        assert!((y_val - 1.0).abs() < 1e-9);
    }

    #[test]
    fn solve_system_rejects_nonlinear_in_single_variable() {
        // x^2 + y = 5,  x + y = 3
        // Probing only at 0/1 would silently treat the first equation as
        // `x + y = 5` (since 1^2 = 1) and produce wrong solutions. The
        // probe-at-2 check catches this: at x=2,y=0 the eqn yields 4,
        // but the linear model would predict 2·1 + 0 = 2.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let two_int = pool.small_int(2);
        let three = pool.small_int(3);
        let five = pool.small_int(5);

        let x2 = pool.pow(x, two_int);
        let lhs1 = pool.add(vec![x2, y]);
        let eq1 = pool.eq_node(lhs1, five);

        let lhs2 = pool.add(vec![x, y]);
        let eq2 = pool.eq_node(lhs2, three);

        let result = solve_system(&mut pool, &[eq1, eq2], &[x, y]);
        match result {
            Err(KernelError::UnsupportedEquation { reason }) => {
                assert!(
                    reason.contains("nonlinear"),
                    "expected nonlinearity reason, got: {}", reason
                );
            }
            other => panic!("expected UnsupportedEquation(nonlinear), got {:?}", other),
        }
    }

    // ---- classify_discriminant ------------------------------------------

    #[test]
    fn classify_discriminant_smallint_signs() {
        let mut pool = ExprPool::new();
        let pos = pool.small_int(7);
        let neg = pool.small_int(-3);
        let zero = pool.zero;
        assert_eq!(classify_discriminant(&pool, pos), Some(DiscSign::Positive));
        assert_eq!(classify_discriminant(&pool, neg), Some(DiscSign::Negative));
        assert_eq!(classify_discriminant(&pool, zero), Some(DiscSign::Zero));
    }

    #[test]
    fn classify_discriminant_huge_bigint_does_not_overflow() {
        // BigInt that exceeds f64::MAX (~1.8e308). `try_to_f64` would return
        // +inf and the old `< 0.0` / `== 0.0` checks would both be false —
        // a false-negative for the `Negative` case below would have caused
        // misclassification through that path. With exact arithmetic, the
        // sign is read directly from the BigInt.
        use num_bigint::BigInt;
        let mut pool = ExprPool::new();
        let huge_pos: BigInt = BigInt::from(10).pow(400);
        let huge_neg = -huge_pos.clone();
        let pos_id = pool.integer(huge_pos);
        let neg_id = pool.integer(huge_neg);
        assert_eq!(classify_discriminant(&pool, pos_id), Some(DiscSign::Positive));
        assert_eq!(classify_discriminant(&pool, neg_id), Some(DiscSign::Negative));
    }

    #[test]
    fn classify_discriminant_tiny_rational_is_not_zero() {
        // Rational 1/10^400 — under f64 this evaluates to 0.0 (underflow).
        // The old code would have hit `disc_val == 0.0` and wrongly fired
        // the double-root path. Exact classification reads the numerator
        // sign and returns `Positive`.
        use num_bigint::BigInt;
        let mut pool = ExprPool::new();
        let tiny_pos = pool.rational(BigInt::from(1), BigInt::from(10).pow(400));
        let tiny_neg = pool.rational(BigInt::from(-1), BigInt::from(10).pow(400));
        assert_eq!(classify_discriminant(&pool, tiny_pos), Some(DiscSign::Positive));
        assert_eq!(classify_discriminant(&pool, tiny_neg), Some(DiscSign::Negative));
    }

    #[test]
    fn classify_discriminant_neg_wrapper_flips_sign() {
        let mut pool = ExprPool::new();
        let five = pool.small_int(5);
        let neg_five = pool.neg(five);
        assert_eq!(classify_discriminant(&pool, neg_five), Some(DiscSign::Negative));
        // Neg(Neg(x)) is collapsed by `pool.neg` to `x`, so we test against a
        // hand-built Neg-of-rational chain too.
        use num_bigint::BigInt;
        let r = pool.rational(BigInt::from(3), BigInt::from(7));
        let neg_r = pool.neg(r);
        assert_eq!(classify_discriminant(&pool, neg_r), Some(DiscSign::Negative));
    }

    #[test]
    fn classify_discriminant_float_uses_f64() {
        let mut pool = ExprPool::new();
        let pos = pool.float(1.5);
        let neg = pool.float(-2.0);
        let zero_pos = pool.float(0.0);
        let zero_neg = pool.float(-0.0);
        assert_eq!(classify_discriminant(&pool, pos), Some(DiscSign::Positive));
        assert_eq!(classify_discriminant(&pool, neg), Some(DiscSign::Negative));
        assert_eq!(classify_discriminant(&pool, zero_pos), Some(DiscSign::Zero));
        // -0.0 must classify as Zero (not Negative) — both IEEE-754 zeros
        // mean the discriminant is zero in finite-precision arithmetic.
        assert_eq!(classify_discriminant(&pool, zero_neg), Some(DiscSign::Zero));
    }

    #[test]
    fn classify_discriminant_returns_none_for_symbolic() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let sym = pool.add(vec![x, two]);
        // Symbolic discriminants are passed through to the two-roots path
        // where a symbolic `sqrt(disc)` is emitted; we must not pretend
        // to know the sign.
        assert_eq!(classify_discriminant(&pool, sym), None);
    }

    #[test]
    fn solve_system_rejects_cross_term() {
        // x·y = 1,  x + y = 3
        // The probe-at-(1,1) check catches the cross-term: at x=1,y=1 the
        // first equation yields 1, but probing each variable in isolation
        // (x=1,y=0) and (x=0,y=1) both yield 0, so the linear model
        // predicts 0 — mismatch.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let three = pool.small_int(3);
        let one = pool.one;

        let xy = pool.mul(vec![x, y]);
        let eq1 = pool.eq_node(xy, one);

        let lhs2 = pool.add(vec![x, y]);
        let eq2 = pool.eq_node(lhs2, three);

        let result = solve_system(&mut pool, &[eq1, eq2], &[x, y]);
        match result {
            Err(KernelError::UnsupportedEquation { reason }) => {
                assert!(
                    reason.contains("cross-term"),
                    "expected cross-term reason, got: {}", reason
                );
            }
            other => panic!("expected UnsupportedEquation(cross-term), got {:?}", other),
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::expr::ExprPool;
    use crate::evalnum::evaluate_numeric;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn linear_solution_satisfies_equation(a in 1i64..20, b in -20i64..20) {
            if a == 0 { return Ok(()); }
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let zero = pool.zero;
            // a*x + b = 0
            let a_int = pool.small_int(a);
            let b_int = pool.small_int(b);
            let ax = pool.mul(vec![a_int, x]);
            let poly = pool.add(vec![ax, b_int]);
            let eq = pool.eq_node(poly, zero);
            let result = solve(&mut pool, eq, x).unwrap();
            prop_assert_eq!(result.solutions.len(), 1);
            let (_, val) = result.solutions[0][0];
            // a*val + b ≈ 0
            let val_f = evaluate_numeric(&pool, &[], val).unwrap();
            let residual = (a as f64) * val_f + (b as f64);
            prop_assert!(residual.abs() < 1e-9, "residual = {}", residual);
        }

        #[test]
        fn quadratic_with_real_roots(
            p in -10i64..0i64,  // negative product → guaranteed real roots
            q in 1i64..10i64,
        ) {
            // (x - p)(x - q) = x^2 - (p+q)x + p*q = 0; roots are p and q.
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let zero = pool.zero;
            let sum = pool.small_int(p + q);
            let prod = pool.small_int(p * q);
            let two_int = pool.small_int(2);
            let x2 = pool.pow(x, two_int);
            let neg_sum = pool.neg(sum);
            let neg_sum_x = pool.mul(vec![neg_sum, x]);
            let poly = pool.add(vec![x2, neg_sum_x, prod]);
            let eq = pool.eq_node(poly, zero);
            let result = solve(&mut pool, eq, x).unwrap();
            prop_assert!(!result.has_complex_roots);
            prop_assert_eq!(result.solutions.len(), 2);
        }
    }
}
