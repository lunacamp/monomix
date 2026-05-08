use crate::error::KernelError;
use crate::expr::{ExprId, ExprNode, ExprPool};
use crate::poly::{coeff, deg};
use crate::simplify::{simplify, SimplifierConfig, SimplifyCache};

pub type Substitution = Vec<(ExprId, ExprId)>;

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
            // Constant — either always true (0=0) or never (c=0 for c!=0)
            Ok(SolutionSet { solutions: vec![], has_complex_roots: false })
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

    if let Some(disc_val) = try_to_f64(pool, disc_simplified) {
        if disc_val < 0.0 {
            return Ok(SolutionSet { solutions: vec![], has_complex_roots: true });
        }
        if disc_val == 0.0 {
            // One root (double): x = -b / (2a)
            let two_int2 = pool.small_int(2);
            let two_a = pool.mul(vec![two_int2, a]);
            let neg_b_local = pool.neg(b);
            let val = pool.div(neg_b_local, two_a);
            let s = simplify(pool, val, &config, &mut cache);
            return Ok(SolutionSet {
                solutions: vec![vec![(var, s)], vec![(var, s)]],
                has_complex_roots: false,
            });
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

/// Solve a linear n x n system of equations (numeric coefficients only) via
/// Gaussian elimination with partial pivoting.
///
/// Each equation must be `Eq(lhs, rhs)` (or a bare expression treated as
/// `expr = 0`). Coefficients are extracted via numeric evaluation:
///   - constant term `b` = E(all vars = 0)
///   - coefficient `a_j` = E(x_j=1, others=0) - b
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

    let mut mat: Vec<Vec<f64>> = Vec::with_capacity(n);
    for &eq in equations {
        let (lhs, rhs) = match pool.get(eq) {
            ExprNode::Eq(l, r) => (*l, *r),
            _ => {
                let z = pool.zero;
                (eq, z)
            }
        };
        let rhs_neg = pool.neg(rhs);
        let poly_expr = pool.add(vec![lhs, rhs_neg]);

        let const_val = evaluate_numeric(pool, &zero_bindings, poly_expr)
            .map_err(|_| KernelError::UnsupportedEquation {
                reason: "non-numeric coefficient in linear system".to_string(),
            })?;

        let mut row = Vec::with_capacity(n + 1);
        for j in 0..n {
            let mut bj = zero_bindings.clone();
            bj[j].1 = 1.0;
            let ej = evaluate_numeric(pool, &bj, poly_expr)
                .map_err(|_| KernelError::UnsupportedEquation {
                    reason: "non-numeric coefficient in linear system".to_string(),
                })?;
            row.push(ej - const_val);
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
