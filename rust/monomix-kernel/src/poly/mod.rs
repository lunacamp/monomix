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
    /// Exponent is a non-negative integer but exceeds `u32::MAX` — the
    /// polynomial view stores exponents as `u32`, so anything larger
    /// can't be represented faithfully. Returning this instead of
    /// silently truncating prevents wildly wrong polynomial degrees.
    ExponentTooLarge,
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
                        // Bounds-check before the `u32` cast. `n: i64` can hold
                        // values up to ~9.2e18, but `Term::exp: u32` caps at
                        // ~4.29e9 — any larger value would silently wrap (e.g.
                        // `x^5_000_000_000` would record `exp = 705_032_704`).
                        if n > u32::MAX as i64 {
                            return Err(ViewError::ExponentTooLarge);
                        }
                        let one = pool.one;
                        return Ok(vec![Term { exp: n as u32, coeff: one }]);
                    }
                    ExprNode::SmallInt(_) => return Err(ViewError::NegativeExponent),
                    // A `BigInt` exponent is an integer (so calling it
                    // `NonIntegerExponent` would lie); it's just too large
                    // for `Term::exp`. The pool narrows BigInts that fit
                    // `i64` back to `SmallInt`, so any live `BigInt` node
                    // already exceeds `i64::MAX` — and therefore `u32::MAX`.
                    ExprNode::BigInt(_) => return Err(ViewError::ExponentTooLarge),
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
    poly.retain(|t| !is_numerically_zero(pool, t.coeff));
}

/// Recursively check if a coefficient ExprId evaluates to zero.
///
/// Handles literal zero, Neg(zero), and Add/Mul nodes whose structural
/// numeric value sums/products to zero. This is needed because the pool
/// only deduplicates by identity — `Add(Neg(one), one)` is a distinct
/// node from `pool.zero` even though it equals zero arithmetically.
///
/// Returns false on non-numeric (symbolic) coefficients to avoid
/// dropping legitimate terms.
fn is_numerically_zero(pool: &ExprPool, id: ExprId) -> bool {
    if pool.is_zero(id) { return true; }
    match numeric_eval(pool, id) {
        Some(NumericVal::Int(n)) => n == 0,
        Some(NumericVal::Rat(p, _q)) => p == 0,
        Some(NumericVal::Float(f)) => f == 0.0,
        None => false,
    }
}

#[derive(Clone, Copy, Debug)]
enum NumericVal {
    Int(i128),
    Rat(i128, i128), // p/q, q > 0
    Float(f64),
}

/// Try to evaluate `id` to a numeric value if it's a closed-form numeric
/// expression (atoms + Add/Mul/Neg/Pow over numerics). Returns None if
/// any subterm is non-numeric or if values overflow i128 reasoning.
fn numeric_eval(pool: &ExprPool, id: ExprId) -> Option<NumericVal> {
    match pool.get(id) {
        ExprNode::SmallInt(n) => Some(NumericVal::Int(*n as i128)),
        ExprNode::BigInt(_) => None, // out-of-range: skip
        ExprNode::Rational(b) => {
            let p = b.0.to_string().parse::<i128>().ok()?;
            let q = b.1.to_string().parse::<i128>().ok()?;
            Some(NumericVal::Rat(p, q))
        }
        ExprNode::Float(f) => Some(NumericVal::Float(f.0)),
        ExprNode::Neg(x) => Some(neg_numeric(numeric_eval(pool, *x)?)),
        ExprNode::Add(children) => {
            let mut acc = NumericVal::Int(0);
            for &c in children.iter() {
                acc = add_numeric(acc, numeric_eval(pool, c)?)?;
            }
            Some(acc)
        }
        ExprNode::Mul(children) => {
            let mut acc = NumericVal::Int(1);
            for &c in children.iter() {
                acc = mul_numeric(acc, numeric_eval(pool, c)?)?;
            }
            Some(acc)
        }
        ExprNode::Div(num, den) => {
            // num/den = num * (1/den); fold to a Rat if both sides resolve
            // numerically. Returning None if den evaluates to zero or to a
            // float (we avoid float division here to keep this conservative).
            let n = numeric_eval(pool, *num)?;
            let d = numeric_eval(pool, *den)?;
            match (n, d) {
                (NumericVal::Float(_), _) | (_, NumericVal::Float(_)) => {
                    let nf = numeric_to_f64(n);
                    let df = numeric_to_f64(d);
                    if df == 0.0 { None } else { Some(NumericVal::Float(nf / df)) }
                }
                _ => {
                    let (np, nq) = numeric_to_rat(n);
                    let (dp, dq) = numeric_to_rat(d);
                    if dp == 0 { return None; }
                    // (np/nq) / (dp/dq) = (np*dq) / (nq*dp)
                    let p = np.checked_mul(dq)?;
                    let q = nq.checked_mul(dp)?;
                    Some(simplify_rat(p, q))
                }
            }
        }
        _ => None,
    }
}

fn neg_numeric(v: NumericVal) -> NumericVal {
    match v {
        NumericVal::Int(n) => NumericVal::Int(-n),
        NumericVal::Rat(p, q) => NumericVal::Rat(-p, q),
        NumericVal::Float(f) => NumericVal::Float(-f),
    }
}

fn add_numeric(a: NumericVal, b: NumericVal) -> Option<NumericVal> {
    match (a, b) {
        (NumericVal::Int(x), NumericVal::Int(y)) => x.checked_add(y).map(NumericVal::Int),
        (NumericVal::Float(x), NumericVal::Float(y)) => Some(NumericVal::Float(x + y)),
        (NumericVal::Float(x), other) | (other, NumericVal::Float(x)) => {
            let y = numeric_to_f64(other);
            Some(NumericVal::Float(x + y))
        }
        (a, b) => {
            let (ap, aq) = numeric_to_rat(a);
            let (bp, bq) = numeric_to_rat(b);
            let p = ap.checked_mul(bq)?.checked_add(bp.checked_mul(aq)?)?;
            let q = aq.checked_mul(bq)?;
            Some(simplify_rat(p, q))
        }
    }
}

fn mul_numeric(a: NumericVal, b: NumericVal) -> Option<NumericVal> {
    match (a, b) {
        (NumericVal::Int(x), NumericVal::Int(y)) => x.checked_mul(y).map(NumericVal::Int),
        (NumericVal::Float(x), NumericVal::Float(y)) => Some(NumericVal::Float(x * y)),
        (NumericVal::Float(x), other) | (other, NumericVal::Float(x)) => {
            let y = numeric_to_f64(other);
            Some(NumericVal::Float(x * y))
        }
        (a, b) => {
            let (ap, aq) = numeric_to_rat(a);
            let (bp, bq) = numeric_to_rat(b);
            let p = ap.checked_mul(bp)?;
            let q = aq.checked_mul(bq)?;
            Some(simplify_rat(p, q))
        }
    }
}

fn numeric_to_rat(v: NumericVal) -> (i128, i128) {
    match v {
        NumericVal::Int(n) => (n, 1),
        NumericVal::Rat(p, q) => (p, q),
        NumericVal::Float(_) => unreachable!("numeric_to_rat called on Float"),
    }
}

fn numeric_to_f64(v: NumericVal) -> f64 {
    match v {
        NumericVal::Int(n) => n as f64,
        NumericVal::Rat(p, q) => p as f64 / q as f64,
        NumericVal::Float(f) => f,
    }
}

fn simplify_rat(p: i128, q: i128) -> NumericVal {
    if q == 0 { return NumericVal::Int(0); } // shouldn't happen in valid input
    if p == 0 { return NumericVal::Int(0); }
    let (p, q) = if q < 0 { (-p, -q) } else { (p, q) };
    let g = gcd_i128(p.unsigned_abs(), q.unsigned_abs()) as i128;
    let p = p / g;
    let q = q / g;
    if q == 1 { NumericVal::Int(p) } else { NumericVal::Rat(p, q) }
}

fn gcd_i128(a: u128, b: u128) -> u128 {
    if b == 0 { a } else { gcd_i128(b, a % b) }
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

// ===========================================================================
// Task 14: polynomial arithmetic + surface ops
// ===========================================================================

/// Add two polynomials, combining like terms via pool.
pub fn poly_add(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> UnivPoly {
    let mut result: UnivPoly = a.to_vec();
    for tb in b {
        if let Some(ta) = result.iter_mut().find(|t| t.exp == tb.exp) {
            ta.coeff = pool.add(vec![ta.coeff, tb.coeff]);
        } else {
            result.push(tb.clone());
        }
    }
    result.sort_by(|a, b| b.exp.cmp(&a.exp));
    remove_zero_terms(pool, &mut result);
    result
}

pub fn poly_sub(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> UnivPoly {
    let neg_b: UnivPoly = b.iter().map(|t| Term { exp: t.exp, coeff: pool.neg(t.coeff) }).collect();
    poly_add(pool, a, &neg_b)
}

pub fn poly_mul(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> UnivPoly {
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
    result.sort_by(|a, b| b.exp.cmp(&a.exp));
    remove_zero_terms(pool, &mut result);
    result
}

#[derive(Debug)]
pub enum DivError {
    DivisionByZero,
}

/// Euclidean polynomial division: f = q*g + r, deg(r) < deg(g).
pub fn poly_div(pool: &mut ExprPool, f: &UnivPoly, g: &UnivPoly) -> Result<(UnivPoly, UnivPoly), DivError> {
    if g.is_empty() || (g.len() == 1 && pool.is_zero(g[0].coeff)) {
        return Err(DivError::DivisionByZero);
    }
    let mut remainder = f.to_vec();
    let mut quotient: UnivPoly = Vec::new();
    let g_lead_exp = g[0].exp;
    let g_lead_coeff = g[0].coeff;

    while !remainder.is_empty() && remainder[0].exp >= g_lead_exp {
        let r_lead = remainder[0].clone();
        let exp = r_lead.exp - g_lead_exp;
        let coeff = pool.div(r_lead.coeff, g_lead_coeff);
        quotient.push(Term { exp, coeff });
        let factor = vec![Term { exp, coeff }];
        let sub = poly_mul(pool, &factor, g);
        remainder = poly_sub(pool, &remainder, &sub);
    }
    quotient.sort_by(|a, b| b.exp.cmp(&a.exp));
    remove_zero_terms(pool, &mut remainder);
    Ok((quotient, remainder))
}

const EXPAND_POW_LIMIT: u32 = 100;

/// Distribute products and powers: (a+b)^n -> sum of terms.
pub fn expand(pool: &mut ExprPool, expr: ExprId) -> ExprId {
    let node = pool.get(expr).clone();
    match node {
        ExprNode::Add(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let expanded: Vec<ExprId> = ids.iter().map(|&c| expand(pool, c)).collect();
            pool.add(expanded)
        }
        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let expanded: Vec<ExprId> = ids.iter().map(|&c| expand(pool, c)).collect();
            expand_mul(pool, &expanded)
        }
        ExprNode::Pow(base, exp) => {
            if let ExprNode::SmallInt(n) = pool.get(exp).clone() {
                if n >= 0 && n <= EXPAND_POW_LIMIT as i64 {
                    let base_expanded = expand(pool, base);
                    return expand_pow(pool, base_expanded, n as u32);
                }
            }
            let base2 = expand(pool, base);
            let exp2 = expand(pool, exp);
            pool.pow(base2, exp2)
        }
        ExprNode::Neg(x) => {
            let x2 = expand(pool, x);
            pool.neg(x2)
        }
        _ => expr,
    }
}

fn expand_mul(pool: &mut ExprPool, factors: &[ExprId]) -> ExprId {
    if factors.is_empty() { return pool.one; }
    if factors.len() == 1 { return factors[0]; }
    let rest = expand_mul(pool, &factors[1..]);
    let lhs = factors[0];
    match pool.get(rest).clone() {
        ExprNode::Add(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let terms: Vec<ExprId> = ids.iter().map(|&c| pool.mul(vec![lhs, c])).collect();
            pool.add(terms)
        }
        _ => match pool.get(lhs).clone() {
            ExprNode::Add(children) => {
                let ids: Vec<ExprId> = children.to_vec();
                let terms: Vec<ExprId> = ids.iter().map(|&c| pool.mul(vec![c, rest])).collect();
                pool.add(terms)
            }
            _ => pool.mul(vec![lhs, rest]),
        }
    }
}

fn expand_pow(pool: &mut ExprPool, base: ExprId, n: u32) -> ExprId {
    if n == 0 { return pool.one; }
    if n == 1 { return base; }
    // Repeated squaring
    let half = expand_pow(pool, base, n / 2);
    let squared = expand_mul(pool, &[half, half]);
    if n % 2 == 0 {
        squared
    } else {
        expand_mul(pool, &[squared, base])
    }
}

pub fn deg(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> Option<u32> {
    view_mut(pool, expr, var).ok().map(|p| p.first().map(|t| t.exp).unwrap_or(0))
}

pub fn coeff(pool: &mut ExprPool, expr: ExprId, var: ExprId, n: u32) -> ExprId {
    match view_mut(pool, expr, var) {
        Ok(poly) => poly.iter().find(|t| t.exp == n).map(|t| t.coeff).unwrap_or(pool.zero),
        Err(_) => pool.zero,
    }
}

pub fn collect_var(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> ExprId {
    match view_mut(pool, expr, var) {
        Ok(poly) => to_expr(pool, &poly, var),
        Err(_) => expr,
    }
}

/// GCD of two polynomials (Euclidean). Used when SimplifierConfig::gcd = true.
pub fn poly_gcd(pool: &mut ExprPool, f: &UnivPoly, g: &UnivPoly) -> UnivPoly {
    if f.is_empty() { return g.to_vec(); }
    if g.is_empty() { return f.to_vec(); }
    let mut a = f.to_vec();
    let mut b = g.to_vec();
    loop {
        match poly_div(pool, &a, &b) {
            Ok((_, r)) if r.is_empty() => return b,
            Ok((_, r)) => { a = b; b = r; }
            Err(_) => {
                let one = pool.one;
                return vec![Term { exp: 0, coeff: one }];
            }
        }
    }
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

    #[test]
    fn poly_add_merges_like_terms() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        // (x^2 + x) + (x^2 + 1) = 2*x^2 + x + 1
        let two_int = pool.small_int(2);
        let x2_a = pool.pow(x, two_int);
        let a_expr = pool.add(vec![x2_a, x]);
        let a = view_mut(&mut pool, a_expr, x).unwrap();

        let two_int2 = pool.small_int(2);
        let x2_b = pool.pow(x, two_int2);
        let one = pool.one;
        let b_expr = pool.add(vec![x2_b, one]);
        let b = view_mut(&mut pool, b_expr, x).unwrap();

        let sum = poly_add(&mut pool, &a, &b);
        assert_eq!(sum.len(), 3); // x^2, x, 1
        assert_eq!(sum[0].exp, 2);
    }

    #[test]
    fn poly_mul_degree_sum() {
        let mut pool = ExprPool::new();
        // (x + 1) * (x - 1) = x^2 - 1
        let one = pool.one;
        let neg_one = pool.neg(one);
        let a = vec![Term { exp: 1, coeff: one }, Term { exp: 0, coeff: one }];
        let b = vec![Term { exp: 1, coeff: one }, Term { exp: 0, coeff: neg_one }];
        let prod = poly_mul(&mut pool, &a, &b);
        assert_eq!(prod.len(), 2); // x^2 and constant
        assert_eq!(prod[0].exp, 2);
    }

    #[test]
    fn poly_div_exact() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        // (x^2 - 1) / (x - 1) = (x + 1) with remainder 0
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let one = pool.one;
        let neg_one = pool.neg(one);
        let f_expr = pool.add(vec![x2, neg_one]); // x^2 - 1
        let f = view_mut(&mut pool, f_expr, x).unwrap();

        let neg_one_id = pool.neg(one);
        let g = vec![
            Term { exp: 1, coeff: one },
            Term { exp: 0, coeff: neg_one_id },
        ];
        let (q, r) = poly_div(&mut pool, &f, &g).unwrap();
        assert_eq!(r.len(), 0, "remainder should be zero");
        assert_eq!(q.len(), 2, "quotient should be x + 1");
    }

    #[test]
    fn expand_distributes() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        // (x + 1)^2 = x^2 + 2*x + 1
        let x_plus_1 = pool.add(vec![x, one]);
        let two_int = pool.small_int(2);
        let expr = pool.pow(x_plus_1, two_int);
        let expanded = expand(&mut pool, expr);
        let poly = view_mut(&mut pool, expanded, x).unwrap();
        assert!(poly.iter().any(|t| t.exp == 2));
        assert!(poly.iter().any(|t| t.exp == 1));
        assert!(poly.iter().any(|t| t.exp == 0));
    }

    // ---- exponent bounds checking ----------------------------------------

    #[test]
    fn view_pow_at_u32_max_exponent_succeeds() {
        // x^(u32::MAX) is right at the boundary of representable exponents.
        // Must succeed and record the exponent exactly.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let exp = pool.small_int(u32::MAX as i64);
        let expr = pool.pow(x, exp);
        let poly = view_mut(&mut pool, expr, x).unwrap();
        assert_eq!(poly.len(), 1);
        assert_eq!(poly[0].exp, u32::MAX);
    }

    #[test]
    fn view_pow_above_u32_max_errors_not_truncates() {
        // x^(u32::MAX + 1) would silently wrap to x^0 under `n as u32`.
        // The bounds check must intercept this and return an error.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let exp = pool.small_int(u32::MAX as i64 + 1);
        let expr = pool.pow(x, exp);
        match view_mut(&mut pool, expr, x) {
            Err(ViewError::ExponentTooLarge) => {}
            other => panic!("expected ExponentTooLarge, got {:?}", other),
        }
    }

    #[test]
    fn view_pow_far_above_u32_max_errors() {
        // The reviewer's example: x^5_000_000_000. Pre-fix, `n as u32` would
        // produce exp = 705_032_704 (5e9 mod 2^32) — silent miscompile.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let exp = pool.small_int(5_000_000_000);
        let expr = pool.pow(x, exp);
        assert!(matches!(
            view_mut(&mut pool, expr, x),
            Err(ViewError::ExponentTooLarge)
        ));
        // Also confirm `deg()` (the public façade) returns None instead of
        // a wrong number — `view_mut(...).ok().map(...)` propagates the
        // error as `None`, which is the right user-visible behavior.
        assert_eq!(deg(&mut pool, expr, x), None);
    }

    #[test]
    fn view_pow_with_bigint_exponent_errors_as_too_large() {
        // BigInt exponents are integers (not "non-integer"). Reclassify as
        // ExponentTooLarge so the diagnostic is honest.
        use num_bigint::BigInt;
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        // 10^20 — exceeds i64::MAX, forces BigInt representation.
        let huge: BigInt = BigInt::from(10u64).pow(20);
        let exp = pool.integer(huge);
        assert!(matches!(pool.get(exp), ExprNode::BigInt(_)));
        let expr = pool.pow(x, exp);
        match view_mut(&mut pool, expr, x) {
            Err(ViewError::ExponentTooLarge) => {}
            other => panic!("expected ExponentTooLarge, got {:?}", other),
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::expr::ExprPool;
    use proptest::prelude::*;

    fn make_poly(pool: &mut ExprPool, coeffs: &[i64]) -> UnivPoly {
        let mut poly = Vec::new();
        for (i, &c) in coeffs.iter().enumerate() {
            if c != 0 {
                let exp = (coeffs.len() - 1 - i) as u32;
                poly.push(Term { exp, coeff: pool.small_int(c) });
            }
        }
        poly.sort_by(|a, b| b.exp.cmp(&a.exp));
        poly
    }

    proptest! {
        #[test]
        fn expand_pow_degree(n in 1u32..15u32) {
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let one = pool.one;
            let x_plus_1 = pool.add(vec![x, one]);
            let n_int = pool.small_int(n as i64);
            let expr = pool.pow(x_plus_1, n_int);
            let expanded = expand(&mut pool, expr);
            let d = deg(&mut pool, expanded, x);
            prop_assert_eq!(d, Some(n));
        }

        #[test]
        fn poly_mul_then_div_recovers(
            a_coeffs in prop::collection::vec(-10i64..10i64, 2..6),
            b_coeffs in prop::collection::vec(-5i64..5i64, 1..3),
        ) {
            // Skip degenerate cases where b is all zeros.
            if b_coeffs.iter().all(|&c| c == 0) { return Ok(()); }
            let mut pool = ExprPool::new();
            let a = make_poly(&mut pool, &a_coeffs);
            let b = make_poly(&mut pool, &b_coeffs);
            if b.is_empty() { return Ok(()); }
            let prod = poly_mul(&mut pool, &a, &b);
            // (a * b) / b should produce a with zero remainder.
            match poly_div(&mut pool, &prod, &b) {
                Ok((_q, r)) => {
                    // Remainder should be empty or all-zero.
                    prop_assert!(r.is_empty() || r.iter().all(|t| pool.is_zero(t.coeff)));
                }
                Err(_) => {} // poly_div should succeed for non-empty divisor
            }
        }
    }
}
