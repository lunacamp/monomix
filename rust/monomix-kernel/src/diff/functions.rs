// Chain rule: d/dx f(u(x)) = f'(u) * u'(x).

use crate::expr::{ExprId, ExprPool, FnTag};
use crate::diff::table::builtin_derivative;
use rustc_hash::FxHashMap;

pub fn diff_fn(
    pool: &mut ExprPool,
    tag: FnTag,
    args: &[ExprId],
    var: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Option<ExprId> {
    if args.len() != 1 { return None; }
    let u = args[0];
    let du = crate::diff::driver::diff_impl(pool, u, var, cache).ok()?;
    if pool.is_zero(du) { return Some(pool.zero); }
    let df_du = builtin_derivative(pool, tag, u)?;
    if pool.is_one(du) {
        Some(df_du)
    } else {
        Some(pool.mul(vec![df_du, du]))
    }
}
