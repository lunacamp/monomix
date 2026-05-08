use crate::error::KernelError;
use crate::expr::{ExprId, ExprNode, ExprPool};
use crate::poly::{common_univariate, poly_div, poly_gcd, view_mut, to_expr};

/// Simplify a Div node by polynomial division when both num and den are
/// polynomials in a common variable. Returns Ok(simplified) or Err on
/// division-by-zero / indeterminate forms.
pub fn simplify_div(pool: &mut ExprPool, expr: ExprId) -> Result<ExprId, KernelError> {
    let (num, den) = match pool.get(expr).clone() {
        ExprNode::Div(n, d) => (n, d),
        _ => return Ok(expr),
    };

    if pool.is_zero(den) {
        if pool.is_zero(num) {
            return Err(KernelError::IndeterminateForm);
        }
        return Err(KernelError::DivisionByZero { span: None });
    }
    if pool.is_one(den) {
        return Ok(num);
    }

    // Try polynomial GCD cancellation
    if let Some(var) = common_univariate(pool, num, den) {
        if let (Ok(f), Ok(g)) = (view_mut(pool, num, var), view_mut(pool, den, var)) {
            let gcd = poly_gcd(pool, &f, &g);
            // gcd is non-trivial: not empty AND not a single term [1, exp=0]
            let is_unit = gcd.len() == 1 && gcd[0].exp == 0 && pool.is_one(gcd[0].coeff);
            if !gcd.is_empty() && !is_unit {
                let q_num = poly_div(pool, &f, &gcd).map(|(q, _)| q).unwrap_or_else(|_| f.clone());
                let q_den = poly_div(pool, &g, &gcd).map(|(q, _)| q).unwrap_or_else(|_| g.clone());
                let new_num = to_expr(pool, &q_num, var);
                let new_den = to_expr(pool, &q_den, var);
                return Ok(pool.div(new_num, new_den));
            }
        }
    }
    Ok(expr)
}
