// Differentiation of Add, Mul, Div, Pow.

use crate::expr::{ExprId, ExprPool};
use crate::error::KernelError;
use rustc_hash::FxHashMap;

pub fn diff_add(
    pool: &mut ExprPool,
    children: &[ExprId],
    var: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Result<ExprId, KernelError> {
    let diffs: Result<Vec<ExprId>, KernelError> = children.iter()
        .map(|&c| crate::diff::driver::diff_impl(pool, c, var, cache))
        .collect();
    Ok(pool.add(diffs?))
}

pub fn diff_mul(
    pool: &mut ExprPool,
    children: &[ExprId],
    var: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Result<ExprId, KernelError> {
    // n-ary Leibniz: sum over i of (d(children[i])/dx * prod(children[j] for j != i))
    let diffs: Result<Vec<ExprId>, KernelError> = children.iter()
        .map(|&c| crate::diff::driver::diff_impl(pool, c, var, cache))
        .collect();
    let diffs = diffs?;
    let terms: Vec<ExprId> = diffs.iter().enumerate().filter_map(|(i, &di)| {
        if pool.is_zero(di) { return None; }
        let others: Vec<ExprId> = children.iter().enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, &c)| c)
            .collect();
        let prod = if others.is_empty() {
            di
        } else {
            let mut factors = others;
            factors.push(di);
            pool.mul(factors)
        };
        Some(prod)
    }).collect();
    if terms.is_empty() { return Ok(pool.zero); }
    Ok(pool.add(terms))
}

pub fn diff_div(
    pool: &mut ExprPool,
    num: ExprId,
    den: ExprId,
    var: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Result<ExprId, KernelError> {
    let d_num = crate::diff::driver::diff_impl(pool, num, var, cache)?;
    let d_den = crate::diff::driver::diff_impl(pool, den, var, cache)?;
    let t1 = pool.mul(vec![d_num, den]);
    let t2 = pool.mul(vec![num, d_den]);
    let neg_t2 = pool.neg(t2);
    let numerator = pool.add(vec![t1, neg_t2]);
    let two_int = pool.small_int(2);
    let den_sq = pool.pow(den, two_int);
    Ok(pool.div(numerator, den_sq))
}

pub fn diff_pow(
    pool: &mut ExprPool,
    base: ExprId,
    exp: ExprId,
    var: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Result<ExprId, KernelError> {
    let base_has_var = pool.contains_symbol(base, var);
    let exp_has_var = pool.contains_symbol(exp, var);

    match (base_has_var, exp_has_var) {
        (false, false) => Ok(pool.zero),
        (true, false) => {
            // d/dx base^n = n * base^(n-1) * d(base)/dx
            let d_base = crate::diff::driver::diff_impl(pool, base, var, cache)?;
            if pool.is_zero(d_base) { return Ok(pool.zero); }
            let one = pool.one;
            let neg_one = pool.neg(one);
            let new_exp = pool.add(vec![exp, neg_one]);
            let power = pool.pow(base, new_exp);
            Ok(pool.mul(vec![exp, power, d_base]))
        }
        (false, true) => {
            // d/dx a^f(x) = a^f(x) * ln(a) * f'(x)
            let d_exp = crate::diff::driver::diff_impl(pool, exp, var, cache)?;
            if pool.is_zero(d_exp) { return Ok(pool.zero); }
            let ln_base = pool.func(crate::expr::FnTag::Log, vec![base]);
            let pow = pool.pow(base, exp);
            Ok(pool.mul(vec![pow, ln_base, d_exp]))
        }
        (true, true) => {
            // d/dx f^g = f^g * (g' * ln(f) + g * f'/f)
            let d_base = crate::diff::driver::diff_impl(pool, base, var, cache)?;
            let d_exp  = crate::diff::driver::diff_impl(pool, exp, var, cache)?;
            let ln_base = pool.func(crate::expr::FnTag::Log, vec![base]);
            let t1 = pool.mul(vec![d_exp, ln_base]);
            let inner_div = pool.div(d_base, base);
            let t2 = pool.mul(vec![exp, inner_div]);
            let inner = pool.add(vec![t1, t2]);
            let pow = pool.pow(base, exp);
            Ok(pool.mul(vec![pow, inner]))
        }
    }
}
