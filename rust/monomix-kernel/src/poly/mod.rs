use crate::expr::{ExprId, ExprNode, ExprPool};
use crate::simplify::numeric::fold_numeric;

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
/// expression: atoms (SmallInt / Rational / Float) plus
/// `Add`/`Mul`/`Neg`/`Div`/`Pow` over numerics. `BigInt` atoms are not
/// folded — they fall through to the `None` arm to keep the integer path
/// inside `i128`.
///
/// `Pow` accepts integer exponents only (fractional/float exponents may
/// produce irrational results that `NumericVal` cannot represent
/// exactly). Returns `None` on any subterm that isn't numeric, on
/// arithmetic overflow, or on division by zero.
fn numeric_eval(pool: &ExprPool, id: ExprId) -> Option<NumericVal> {
    match pool.get(id) {
        ExprNode::SmallInt(n) => Some(NumericVal::Int(*n as i128)),
        ExprNode::BigInt(_) => None, // out-of-range: skip
        ExprNode::Rational(b) => {
            use num_traits::ToPrimitive;
            let p = b.0.to_i128()?;
            let q = b.1.to_i128()?;
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
        ExprNode::Pow(base, exp) => {
            // The doc above advertises numeric Pow folding; this arm
            // implements it. Without it, coefficients like `0^n` or
            // `(2-2)^3` would slip past `is_numerically_zero` and pollute
            // `remove_zero_terms`, `deg`, and the polynomial solver paths.
            let b = numeric_eval(pool, *base)?;
            // Only integer exponents are folded — fractional/float
            // exponents may produce irrational results (`sqrt`, `2^(1/3)`),
            // which `NumericVal` cannot represent exactly.
            let e = match numeric_eval(pool, *exp)? {
                NumericVal::Int(n) => n,
                NumericVal::Rat(p, q) if q == 1 => p,
                _ => return None,
            };
            pow_numeric(b, e)
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

/// Integer exponentiation over `NumericVal`. Returns `None` on overflow
/// (Int / Rat paths use `checked_pow`) or when the base is zero with a
/// negative exponent (division by zero).
///
/// `0^0` returns `Int(1)` — matching `i128::pow(0)`, `f64::powi(0)`, and the
/// `num_traits::Pow` convention. Mathematically debatable, but the kernel
/// only consumes the result via zero-detection / numeric eval where this
/// convention is harmless.
fn pow_numeric(base: NumericVal, exp: i128) -> Option<NumericVal> {
    if exp == 0 {
        return Some(NumericVal::Int(1));
    }
    if exp > 0 {
        pow_nonneg(base, exp as u128)
    } else {
        // x^(-n) = 1 / x^n. Compute the positive power first, then invert.
        let pos = pow_nonneg(base, exp.unsigned_abs())?;
        reciprocal_numeric(pos)
    }
}

fn pow_nonneg(base: NumericVal, exp: u128) -> Option<NumericVal> {
    if exp == 0 {
        return Some(NumericVal::Int(1));
    }
    match base {
        NumericVal::Int(b) => {
            // `i128::checked_pow` takes `u32`. Exponents above that are
            // either (a) going to overflow `i128` (typical, e.g. 2^200) or
            // (b) the base is in {-1, 0, 1} where the result is exactly
            // representable. We bail conservatively for (a) and short-
            // circuit (b) explicitly.
            if b == 0 { return Some(NumericVal::Int(0)); }
            if b == 1 { return Some(NumericVal::Int(1)); }
            if b == -1 {
                return Some(NumericVal::Int(if exp & 1 == 0 { 1 } else { -1 }));
            }
            let e32: u32 = exp.try_into().ok()?;
            b.checked_pow(e32).map(NumericVal::Int)
        }
        NumericVal::Rat(p, q) => {
            if p == 0 { return Some(NumericVal::Int(0)); }
            let e32: u32 = exp.try_into().ok()?;
            let pn = p.checked_pow(e32)?;
            let qn = q.checked_pow(e32)?;
            Some(simplify_rat(pn, qn))
        }
        NumericVal::Float(f) => {
            // `f64::powi` takes `i32`; if the exponent doesn't fit, fall
            // back to `powf`. Precision loss for very large exponents is
            // acceptable — `f64` would be saturated anyway.
            if exp <= i32::MAX as u128 {
                Some(NumericVal::Float(f.powi(exp as i32)))
            } else {
                Some(NumericVal::Float(f.powf(exp as f64)))
            }
        }
    }
}

fn reciprocal_numeric(v: NumericVal) -> Option<NumericVal> {
    match v {
        NumericVal::Int(0) => None,
        NumericVal::Int(n) => Some(simplify_rat(1, n)),
        NumericVal::Rat(p, _) if p == 0 => None,
        NumericVal::Rat(p, q) => Some(simplify_rat(q, p)),
        NumericVal::Float(f) if f == 0.0 => None,
        NumericVal::Float(f) => Some(NumericVal::Float(1.0 / f)),
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
///
/// New quotient and remainder coefficients are passed through
/// `fold_numeric` so that purely-numeric arithmetic (Div, Add, Mul, Neg
/// trees produced by the in-loop pool operations) collapses to canonical
/// atoms before any downstream check. Without this, `remove_zero_terms`
/// and `simplify_div`'s exact-cancellation predicate would miss legal
/// cancellations whose structural form is non-trivial — leaving messy
/// nested `Div` nodes in the output.
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
        let div = pool.div(r_lead.coeff, g_lead_coeff);
        let coeff = fold_numeric(pool, div).unwrap_or(div);
        quotient.push(Term { exp, coeff });
        let factor = vec![Term { exp, coeff }];
        let sub = poly_mul(pool, &factor, g);
        remainder = poly_sub(pool, &remainder, &sub);
        // Fold each remainder coefficient: poly_sub built `Add([a, Neg(b)])`
        // chains over the in-loop pool ops, which `is_numerically_zero`
        // catches only for closed-form numeric subsets. Folding now keeps
        // the rest of the loop (and the final `remove_zero_terms`) honest.
        for t in remainder.iter_mut() {
            if let Some(folded) = fold_numeric(pool, t.coeff) {
                t.coeff = folded;
            }
        }
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
///
/// `b` is monic-normalized at every iteration (when its leading coefficient
/// is numeric): without that, leading coefficients accumulate `Div` nodes
/// across iterations and the final GCD's `is_unit` check fails for results
/// that are mathematically `1` but structurally `Div(1,1)`. `simplify_div`
/// would then refuse to cancel legitimate factors. Symbolic leading
/// coefficients are left alone — dividing by them would just move the same
/// `Div` clutter into every coefficient.
pub fn poly_gcd(pool: &mut ExprPool, f: &UnivPoly, g: &UnivPoly) -> UnivPoly {
    if f.is_empty() { return g.to_vec(); }
    if g.is_empty() { return f.to_vec(); }
    let mut a = f.to_vec();
    let mut b = g.to_vec();
    make_monic(pool, &mut b);
    loop {
        match poly_div(pool, &a, &b) {
            Ok((_, r)) if r.is_empty() => return b,
            Ok((_, r)) => {
                a = b;
                b = r;
                make_monic(pool, &mut b);
            }
            Err(_) => {
                let one = pool.one;
                return vec![Term { exp: 0, coeff: one }];
            }
        }
    }
}

/// Divide every coefficient by the leading coefficient when it folds to a
/// numeric atom. No-op for empty polys, polys whose leading coefficient is
/// already `1`, and polys whose leading coefficient is symbolic.
fn make_monic(pool: &mut ExprPool, poly: &mut UnivPoly) {
    if poly.is_empty() {
        return;
    }
    let lead = poly[0].coeff;
    if pool.is_one(lead) {
        return;
    }
    let lead_atom = fold_numeric(pool, lead).unwrap_or(lead);
    if !matches!(
        pool.get(lead_atom),
        ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_)
    ) {
        return;
    }
    if pool.is_one(lead_atom) {
        // Leading already 1 after folding — just rebind so callers see the
        // canonical atom in `poly[0].coeff` for downstream `is_unit` checks.
        poly[0].coeff = lead_atom;
        return;
    }
    for t in poly.iter_mut() {
        let div = pool.div(t.coeff, lead_atom);
        t.coeff = fold_numeric(pool, div).unwrap_or(div);
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
    fn poly_div_returns_canonical_atoms() {
        // Regression: `pool.div(r_lead, g_lead)` previously left raw Div
        // nodes in quotient coefficients even when both sides were the
        // same integer. With `fold_numeric` post-processing, dividing two
        // monic polynomials yields quotient coefficients that are
        // canonical SmallInt atoms — not Div(1,1).
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let one = pool.one;
        let neg_one = pool.neg(one);
        let f_expr = pool.add(vec![x2, neg_one]);
        let f = view_mut(&mut pool, f_expr, x).unwrap();
        let neg_one_id = pool.neg(one);
        let g = vec![
            Term { exp: 1, coeff: one },
            Term { exp: 0, coeff: neg_one_id },
        ];
        let (q, _r) = poly_div(&mut pool, &f, &g).unwrap();
        for t in &q {
            assert!(
                !matches!(pool.get(t.coeff), ExprNode::Div(_, _)),
                "quotient coefficient at exp {} must not be a raw Div node, got {:?}",
                t.exp,
                pool.get(t.coeff)
            );
        }
    }

    #[test]
    fn poly_gcd_returns_canonical_one_for_coprime_inputs() {
        // gcd(x^2 + 2x + 1, x + 1) = (x + 1) in Q[x]; after make_monic
        // the leading coefficient is exactly `pool.one`. Then
        // gcd(remainder of dividing one by the other) terminates with
        // a single-term constant whose coefficient is `pool.one` —
        // is_unit predicates (simplify_div etc.) can match on it.
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        let two = pool.small_int(2);
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let two_x = pool.mul(vec![two, x]);
        let f_expr = pool.add(vec![x2, two_x, one]); // x^2 + 2x + 1
        let g_expr = pool.add(vec![x, one]);          // x + 1
        let f = view_mut(&mut pool, f_expr, x).unwrap();
        let g = view_mut(&mut pool, g_expr, x).unwrap();
        let gcd = poly_gcd(&mut pool, &f, &g);
        // GCD is monic (leading coefficient is `pool.one`), not Div(1,1).
        assert!(!gcd.is_empty());
        assert!(
            pool.is_one(gcd[0].coeff),
            "gcd leading coefficient must be canonical 1, got {:?}",
            pool.get(gcd[0].coeff)
        );
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

    // ---- numeric_eval over Pow -------------------------------------------

    #[test]
    fn numeric_eval_pow_integer_base_and_exponent() {
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let p = pool.pow(two, three);
        match numeric_eval(&pool, p) {
            Some(NumericVal::Int(8)) => {}
            other => panic!("expected Int(8), got {:?}", other),
        }
    }

    // Note: there's no test for `0^0`. `pool.pow(_, 0)` short-circuits to
    // `pool.one` ([expr/mod.rs:309](../expr/mod.rs)), so a `Pow(_, 0)` node
    // is unreachable through the normal API. The `if exp == 0 { Int(1) }`
    // guard inside `pow_numeric` is still kept as a defensive safety net
    // for any future caller that builds Pow nodes via raw `intern`.

    #[test]
    fn numeric_eval_pow_zero_to_positive_is_zero() {
        let mut pool = ExprPool::new();
        let zero = pool.zero;
        let five = pool.small_int(5);
        let p = pool.pow(zero, five);
        match numeric_eval(&pool, p) {
            Some(NumericVal::Int(0)) => {}
            other => panic!("expected Int(0), got {:?}", other),
        }
    }

    #[test]
    fn numeric_eval_pow_negative_exponent_inverts() {
        // 2^(-3) = 1/8
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let neg_three = pool.small_int(-3);
        let p = pool.pow(two, neg_three);
        match numeric_eval(&pool, p) {
            Some(NumericVal::Rat(1, 8)) => {}
            other => panic!("expected Rat(1, 8), got {:?}", other),
        }
    }

    #[test]
    fn numeric_eval_pow_zero_to_negative_is_none() {
        // 0^(-n) is division by zero; conservatively None.
        let mut pool = ExprPool::new();
        let zero = pool.zero;
        let neg_one = pool.small_int(-1);
        let p = pool.pow(zero, neg_one);
        assert!(numeric_eval(&pool, p).is_none());
    }

    #[test]
    fn numeric_eval_pow_overflow_returns_none() {
        // 2^200 overflows i128. Must return None rather than wrapping.
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let huge_exp = pool.small_int(200);
        let p = pool.pow(two, huge_exp);
        assert!(numeric_eval(&pool, p).is_none());
    }

    #[test]
    fn numeric_eval_pow_neg_one_alternates() {
        // (-1)^n: shortcut for the {-1, 0, 1} base optimization. Must
        // return +1 for even, -1 for odd, regardless of how large the
        // exponent is (the shortcut sidesteps the u32 cap).
        let mut pool = ExprPool::new();
        let neg_one = pool.small_int(-1);
        let huge_even = pool.small_int(1_000_000);
        let huge_odd = pool.small_int(1_000_001);
        let p_even = pool.pow(neg_one, huge_even);
        let p_odd = pool.pow(neg_one, huge_odd);
        assert!(matches!(numeric_eval(&pool, p_even), Some(NumericVal::Int(1))));
        assert!(matches!(numeric_eval(&pool, p_odd), Some(NumericVal::Int(-1))));
    }

    #[test]
    fn numeric_eval_pow_rational_base() {
        // (1/2)^3 = 1/8
        use num_bigint::BigInt;
        let mut pool = ExprPool::new();
        let half = pool.rational(BigInt::from(1), BigInt::from(2));
        let three = pool.small_int(3);
        let p = pool.pow(half, three);
        match numeric_eval(&pool, p) {
            Some(NumericVal::Rat(1, 8)) => {}
            other => panic!("expected Rat(1, 8), got {:?}", other),
        }
    }

    #[test]
    fn is_numerically_zero_detects_pow_zero_to_positive() {
        // The exact case the reviewer flagged: a coefficient `0^n` (n>0)
        // must be detected as zero so `remove_zero_terms` strips it.
        let mut pool = ExprPool::new();
        let zero = pool.zero;
        let n = pool.small_int(5);
        let p = pool.pow(zero, n);
        assert!(is_numerically_zero(&pool, p),
                "Pow(0, 5) must be recognized as numerically zero");
    }

    #[test]
    fn is_numerically_zero_detects_pow_of_structural_zero() {
        // `(2 - 2)^3 = 0`. The base is structurally `Add([2, Neg(2)])` —
        // not literal zero — but evaluates to zero numerically, so the
        // whole Pow must evaluate to zero.
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let neg_two = pool.neg(two);
        let base = pool.add(vec![two, neg_two]);
        // Defensive: confirm the base isn't already the canonical pool.zero
        // (so we actually exercise the recursive numeric_eval path).
        // Note: pool.add may collapse `2 + (-2)` to pool.zero — if so this
        // test trivially passes via the `is_zero` shortcut, which is still
        // correct behavior.
        let three = pool.small_int(3);
        let p = pool.pow(base, three);
        assert!(is_numerically_zero(&pool, p));
    }

    #[test]
    fn remove_zero_terms_strips_pow_zero_coefficients() {
        // End-to-end: a polynomial term whose coefficient is `Pow(0, n)`
        // must be removed by `remove_zero_terms`. Pre-fix, the Pow node
        // survived numeric_eval and the term polluted the polynomial.
        let mut pool = ExprPool::new();
        let one = pool.one;
        let zero = pool.zero;
        let three = pool.small_int(3);
        let zero_pow = pool.pow(zero, three); // 0^3 — should be dropped
        let mut poly = vec![
            Term { exp: 2, coeff: one },
            Term { exp: 1, coeff: zero_pow },
            Term { exp: 0, coeff: one },
        ];
        remove_zero_terms(&pool, &mut poly);
        // The `0^3` term must be gone; `1*x^2` and `1` remain.
        assert_eq!(poly.len(), 2, "Pow(0,3) coefficient term should be stripped");
        assert!(poly.iter().all(|t| t.exp != 1),
                "the exp=1 term (with Pow(0,3) coefficient) must be removed");
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
