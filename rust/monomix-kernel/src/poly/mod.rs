use crate::expr::{ExprId, ExprNode, ExprPool};

#[derive(Clone, Debug)]
pub struct Term {
    pub exp: u32,
    pub coeff: ExprId,
}

pub type UnivPoly = Vec<Term>;

#[derive(Debug)]
pub enum ViewError {
    NonPolynomialSubterm { reason: &'static str },
    NonIntegerExponent,
    NegativeExponent,
    DivisionByVariable,
}

/// Attempt to view `expr` as a univariate polynomial in `var`.
/// Requires `&mut pool` because constructing coefficients from `Neg` or
/// `Div` involves interning new nodes via the normalizing constructors.
pub fn view(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> Result<UnivPoly, ViewError> {
    view_mut_impl(pool, expr, var)
}

/// Alias retained for spec-vocabulary consistency. Same as `view`.
pub fn view_mut(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> Result<UnivPoly, ViewError> {
    view_mut_impl(pool, expr, var)
}

fn view_mut_impl(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> Result<UnivPoly, ViewError> {
    if expr == var {
        let one = pool.one;
        return Ok(vec![Term { exp: 1, coeff: one }]);
    }
    let node = pool.get(expr).clone();
    match node {
        ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_)
        | ExprNode::Float(_) | ExprNode::Symbol(_) => {
            Ok(vec![Term { exp: 0, coeff: expr }])
        }
        ExprNode::Neg(inner) => {
            let mut poly = view_mut_impl(pool, inner, var)?;
            for t in &mut poly {
                t.coeff = pool.neg(t.coeff);
            }
            remove_zero_terms(pool, &mut poly);
            Ok(poly)
        }
        ExprNode::Add(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let mut result = UnivPoly::new();
            for child in ids {
                let child_poly = view_mut_impl(pool, child, var)?;
                result = merge_add(pool, result, child_poly);
            }
            remove_zero_terms(pool, &mut result);
            Ok(result)
        }
        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let one = pool.one;
            let mut result = vec![Term { exp: 0, coeff: one }];
            for child in ids {
                let child_poly = view_mut_impl(pool, child, var)?;
                result = merge_mul(pool, &result, &child_poly);
            }
            remove_zero_terms(pool, &mut result);
            Ok(result)
        }
        ExprNode::Pow(base, exp) => {
            if !pool.contains_symbol(base, var) {
                return Ok(vec![Term { exp: 0, coeff: expr }]);
            }
            if base == var {
                match pool.get(exp).clone() {
                    ExprNode::SmallInt(n) if n >= 0 => {
                        let one = pool.one;
                        return Ok(vec![Term { exp: n as u32, coeff: one }]);
                    }
                    ExprNode::SmallInt(_) => return Err(ViewError::NegativeExponent),
                    _ => return Err(ViewError::NonIntegerExponent),
                }
            }
            Err(ViewError::NonPolynomialSubterm { reason: "complex power" })
        }
        ExprNode::Div(num, den) => {
            if pool.contains_symbol(den, var) {
                return Err(ViewError::DivisionByVariable);
            }
            let mut poly = view_mut_impl(pool, num, var)?;
            for t in &mut poly {
                t.coeff = pool.div(t.coeff, den);
            }
            Ok(poly)
        }
        _ => {
            if pool.contains_symbol(expr, var) {
                Err(ViewError::NonPolynomialSubterm { reason: "complex node" })
            } else {
                Ok(vec![Term { exp: 0, coeff: expr }])
            }
        }
    }
}

fn remove_zero_terms(pool: &ExprPool, poly: &mut UnivPoly) {
    poly.retain(|t| !pool.is_zero(t.coeff));
}

/// Merge two polys by summing same-exponent terms via `pool.add`.
fn merge_add(pool: &mut ExprPool, mut a: UnivPoly, b: UnivPoly) -> UnivPoly {
    for tb in b {
        if let Some(ta) = a.iter_mut().find(|t| t.exp == tb.exp) {
            ta.coeff = pool.add(vec![ta.coeff, tb.coeff]);
        } else {
            a.push(tb);
        }
    }
    a.sort_by(|x, y| y.exp.cmp(&x.exp));
    a
}

/// Multiply two polys via sparse convolution + pool ops.
fn merge_mul(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> UnivPoly {
    let mut result: UnivPoly = Vec::new();
    for ta in a {
        for tb in b {
            let exp = ta.exp + tb.exp;
            let coeff = pool.mul(vec![ta.coeff, tb.coeff]);
            if let Some(t) = result.iter_mut().find(|t| t.exp == exp) {
                t.coeff = pool.add(vec![t.coeff, coeff]);
            } else {
                result.push(Term { exp, coeff });
            }
        }
    }
    result.sort_by(|x, y| y.exp.cmp(&x.exp));
    result
}

/// Rebuild an ExprId from a UnivPoly: sum of coeff * var^exp terms.
pub fn to_expr(pool: &mut ExprPool, poly: &UnivPoly, var: ExprId) -> ExprId {
    if poly.is_empty() {
        return pool.zero;
    }
    let terms: Vec<ExprId> = poly.iter().map(|t| {
        if t.exp == 0 {
            t.coeff
        } else {
            let exp_id = pool.small_int(t.exp as i64);
            let pow = pool.pow(var, exp_id);
            if pool.is_one(t.coeff) {
                pow
            } else {
                pool.mul(vec![t.coeff, pow])
            }
        }
    }).collect();
    if terms.len() == 1 {
        terms[0]
    } else {
        pool.add(terms)
    }
}

pub fn is_polynomial_in(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> bool {
    view_mut(pool, expr, var).is_ok()
}

pub fn common_univariate(pool: &mut ExprPool, e1: ExprId, e2: ExprId) -> Option<ExprId> {
    let syms = collect_symbols(pool, e1);
    for s in syms {
        if is_polynomial_in(pool, e1, s) && is_polynomial_in(pool, e2, s) {
            return Some(s);
        }
    }
    None
}

fn collect_symbols(pool: &ExprPool, expr: ExprId) -> Vec<ExprId> {
    let mut syms = Vec::new();
    pool.fold(expr, (), &mut |_, id, node| {
        if matches!(node, ExprNode::Symbol(_)) {
            if !syms.contains(&id) { syms.push(id); }
        }
    });
    syms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;

    #[test]
    fn view_linear_poly() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        // 2*x + 3
        let two_x = pool.mul(vec![two, x]);
        let expr = pool.add(vec![two_x, three]);
        let poly = view_mut(&mut pool, expr, x).expect("should view as univariate poly in x");
        assert_eq!(poly.len(), 2);
        // degree 1 term first (sorted descending)
        assert_eq!(poly[0].exp, 1);
        assert_eq!(poly[1].exp, 0);
    }

    #[test]
    fn view_constant_is_poly() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let five = pool.small_int(5);
        let poly = view_mut(&mut pool, five, x).expect("constant is trivially polynomial");
        assert_eq!(poly.len(), 1);
        assert_eq!(poly[0].exp, 0);
    }

    #[test]
    fn is_polynomial_in_true() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let two_x = pool.mul(vec![two, x]);
        assert!(is_polynomial_in(&mut pool, two_x, x));
    }

    #[test]
    fn is_polynomial_in_false_for_division() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        let one_over_x = pool.div(one, x);
        assert!(!is_polynomial_in(&mut pool, one_over_x, x));
    }

    #[test]
    fn to_expr_roundtrip() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let one = pool.one;
        // x^2 + 2*x + 1
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let two_x = pool.mul(vec![two, x]);
        let expr = pool.add(vec![x2, two_x, one]);
        let poly = view_mut(&mut pool, expr, x).expect("should view");
        let reconstructed = to_expr(&mut pool, &poly, x);
        let poly2 = view_mut(&mut pool, reconstructed, x).expect("roundtrip should still view");
        assert_eq!(poly.len(), poly2.len());
    }
}
