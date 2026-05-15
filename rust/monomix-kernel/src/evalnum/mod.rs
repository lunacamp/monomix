use crate::error::KernelError;
use crate::expr::{ExprId, ExprNode, ExprPool, FnTag};
use num_traits::ToPrimitive;

pub type Bindings<'a> = &'a [(ExprId, f64)];

/// Evaluate `expr` to f64 given `bindings` for free symbols.
/// NaN results map to `KernelError::NumericNaN`, never propagated as f64.
pub fn evaluate_numeric(
    pool: &ExprPool,
    bindings: Bindings<'_>,
    expr: ExprId,
) -> Result<f64, KernelError> {
    let result = eval_impl(pool, bindings, expr)?;
    if result.is_nan() {
        return Err(KernelError::NumericNaN);
    }
    Ok(result)
}

fn eval_impl(
    pool: &ExprPool,
    bindings: Bindings<'_>,
    expr: ExprId,
) -> Result<f64, KernelError> {
    match pool.get(expr) {
        ExprNode::SmallInt(n) => Ok(*n as f64),
        ExprNode::BigInt(n) => n.to_f64().ok_or(KernelError::Overflow),
        ExprNode::Rational(b) => {
            let p = b.0.to_f64().ok_or(KernelError::Overflow)?;
            let q = b.1.to_f64().ok_or(KernelError::Overflow)?;
            Ok(p / q)
        }
        ExprNode::Float(f) => Ok(f.0),
        ExprNode::Symbol(s) => {
            // Bindings lookup
            if let Some(&(_, val)) = bindings.iter().find(|(id, _)| *id == expr) {
                return Ok(val);
            }
            // Pre-interned constants
            let name = pool.str_of(*s);
            match name {
                "e" => return Ok(std::f64::consts::E),
                "pi" => return Ok(std::f64::consts::PI),
                _ => {}
            }
            Err(KernelError::UnboundSymbol(name.to_string()))
        }
        ExprNode::Neg(x) => Ok(-eval_impl(pool, bindings, *x)?),
        ExprNode::Add(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let mut sum = 0.0f64;
            for c in ids.iter() {
                sum += eval_impl(pool, bindings, *c)?;
            }
            Ok(sum)
        }
        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let mut prod = 1.0f64;
            for c in ids.iter() {
                prod *= eval_impl(pool, bindings, *c)?;
            }
            Ok(prod)
        }
        ExprNode::Pow(base, exp) => {
            let base = *base;
            let exp = *exp;
            let b = eval_impl(pool, bindings, base)?;
            let e = eval_impl(pool, bindings, exp)?;
            Ok(b.powf(e))
        }
        ExprNode::Div(num, den) => {
            let num = *num;
            let den = *den;
            let n = eval_impl(pool, bindings, num)?;
            let d = eval_impl(pool, bindings, den)?;
            if d == 0.0 {
                if n == 0.0 {
                    return Err(KernelError::IndeterminateForm);
                }
                return Err(KernelError::DivisionByZero { span: None });
            }
            Ok(n / d)
        }
        ExprNode::Fn(tag, args) => {
            let tag = *tag;
            let arg_ids: Vec<ExprId> = args.to_vec();
            if matches!(tag, FnTag::Custom(_)) {
                return Err(KernelError::UnsupportedFn);
            }
            eval_fn(pool, bindings, tag, &arg_ids)
        }
        _ => Err(KernelError::UnsupportedFn),
    }
}

fn eval_fn(
    pool: &ExprPool,
    bindings: Bindings<'_>,
    tag: FnTag,
    args: &[ExprId],
) -> Result<f64, KernelError> {
    if args.len() != 1 {
        return Err(KernelError::UnsupportedFn);
    }
    let v = eval_impl(pool, bindings, args[0])?;
    match tag {
        FnTag::Sin => Ok(v.sin()),
        FnTag::Cos => Ok(v.cos()),
        FnTag::Tan => Ok(v.tan()),
        FnTag::Exp => Ok(v.exp()),
        FnTag::Log => {
            if v <= 0.0 {
                return Err(KernelError::LogOfNonPositive);
            }
            Ok(v.ln())
        }
        FnTag::Sqrt => {
            if v < 0.0 {
                return Err(KernelError::SqrtOfNegative);
            }
            Ok(v.sqrt())
        }
        FnTag::Abs => Ok(v.abs()),
        FnTag::Asin => {
            if !(-1.0..=1.0).contains(&v) {
                return Err(KernelError::DomainError { fn_name: "asin" });
            }
            Ok(v.asin())
        }
        FnTag::Acos => {
            if !(-1.0..=1.0).contains(&v) {
                return Err(KernelError::DomainError { fn_name: "acos" });
            }
            Ok(v.acos())
        }
        FnTag::Atan => Ok(v.atan()),
        FnTag::Custom(_) => Err(KernelError::UnsupportedFn),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KernelError;
    use crate::expr::ExprPool;

    #[test]
    fn eval_integer_literal() {
        let mut pool = ExprPool::new();
        let five = pool.small_int(5);
        let result = evaluate_numeric(&pool, &[], five).unwrap();
        assert_eq!(result, 5.0);
    }

    #[test]
    fn eval_bound_symbol() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let result = evaluate_numeric(&pool, &[(x, 3.0)], x).unwrap();
        assert_eq!(result, 3.0);
    }

    #[test]
    fn eval_unbound_symbol_errors() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let result = evaluate_numeric(&pool, &[], x);
        assert!(matches!(result, Err(KernelError::UnboundSymbol(_))));
    }

    #[test]
    fn eval_add() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let sum = pool.add(vec![x, y]);
        let result = evaluate_numeric(&pool, &[(x, 2.0), (y, 3.0)], sum).unwrap();
        assert_eq!(result, 5.0);
    }

    #[test]
    fn eval_sin() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let sin_x = pool.func(crate::expr::FnTag::Sin, vec![x]);
        let result = evaluate_numeric(&pool, &[(x, 0.0)], sin_x).unwrap();
        assert!((result - 0.0).abs() < 1e-10);
    }

    #[test]
    fn eval_log_nonpositive_errors() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let log_x = pool.func(crate::expr::FnTag::Log, vec![x]);
        let result = evaluate_numeric(&pool, &[(x, -1.0)], log_x);
        assert!(matches!(result, Err(KernelError::LogOfNonPositive)));
    }

    #[test]
    fn evalnum_bool_const_is_unsupported() {
        let mut pool = ExprPool::new();
        let t = pool.bool_const(true);
        let result = evaluate_numeric(&pool, &[], t);
        assert!(matches!(result, Err(KernelError::UnsupportedFn)));
    }

    #[test]
    fn eval_nan_errors() {
        let mut pool = ExprPool::new();
        let _x = pool.symbol("x");
        // 0/0 yields IndeterminateForm before reaching NaN
        let zero = pool.zero;
        let zero_div = pool.div(zero, zero);
        let result = evaluate_numeric(&pool, &[], zero_div);
        assert!(result.is_err());
    }
}
