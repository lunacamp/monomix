// Recursive descent differentiation driver, with per-call DiffCache.

use crate::expr::{ExprId, ExprNode, ExprPool, FnTag};
use crate::error::KernelError;
use crate::diff::{arith, functions};
use rustc_hash::FxHashMap;

pub type DiffCache = FxHashMap<ExprId, ExprId>;

pub fn diff_impl(
    pool: &mut ExprPool,
    expr: ExprId,
    var: ExprId,
    cache: &mut DiffCache,
) -> Result<ExprId, KernelError> {
    if let Some(&cached) = cache.get(&expr) {
        return Ok(cached);
    }
    let result = diff_inner(pool, expr, var, cache)?;
    cache.insert(expr, result);
    Ok(result)
}

fn diff_inner(
    pool: &mut ExprPool,
    expr: ExprId,
    var: ExprId,
    cache: &mut DiffCache,
) -> Result<ExprId, KernelError> {
    if expr == var {
        return Ok(pool.one);
    }
    let node = pool.get(expr).clone();
    match node {
        ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_)
        | ExprNode::Float(_) => Ok(pool.zero),
        ExprNode::Symbol(_) => Ok(pool.zero), // different symbol
        ExprNode::Eq(_, _) => Err(KernelError::DifferentiateEquation),

        ExprNode::Neg(x) => {
            let dx = diff_impl(pool, x, var, cache)?;
            Ok(pool.neg(dx))
        }

        ExprNode::Add(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            arith::diff_add(pool, &ids, var, cache)
        }

        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            arith::diff_mul(pool, &ids, var, cache)
        }

        ExprNode::Div(num, den) => arith::diff_div(pool, num, den, var, cache),

        ExprNode::Pow(base, exp) => arith::diff_pow(pool, base, exp, var, cache),

        ExprNode::Fn(tag, args) => {
            let arg_ids: Vec<ExprId> = args.to_vec();
            if let Some(result) = functions::diff_fn(pool, tag, &arg_ids, var, cache) {
                Ok(result)
            } else {
                // Unknown function — symbolic placeholder df(original, var)
                let df_name = pool.intern_str_pub("df");
                let placeholder = pool.func(FnTag::Custom(df_name), vec![expr, var]);
                Ok(placeholder)
            }
        }

        ExprNode::List(_) | ExprNode::String(_) => Ok(pool.zero),

        ExprNode::Lt(_, _)
        | ExprNode::Le(_, _)
        | ExprNode::Gt(_, _)
        | ExprNode::Ge(_, _)
        | ExprNode::Not(_)
        | ExprNode::And(_)
        | ExprNode::Or(_)
        | ExprNode::Implies(_, _)
        | ExprNode::BoolConst(_) => Err(KernelError::UnsupportedEquation {
            reason: "boolean/comparison expressions are not differentiable".to_string(),
        }),
    }
}
