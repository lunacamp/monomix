// Built-in derivative table: d/du f(u) — chain-rule applied separately by caller.

use crate::expr::{ExprId, ExprPool, FnTag};

pub fn builtin_derivative(pool: &mut ExprPool, tag: FnTag, u: ExprId) -> Option<ExprId> {
    match tag {
        FnTag::Sin  => Some(pool.func(FnTag::Cos, vec![u])),
        FnTag::Cos  => {
            let sin_u = pool.func(FnTag::Sin, vec![u]);
            Some(pool.neg(sin_u))
        }
        FnTag::Tan  => {
            let cos_u = pool.func(FnTag::Cos, vec![u]);
            let two_int = pool.small_int(2);
            let cos2 = pool.pow(cos_u, two_int);
            let one = pool.one;
            Some(pool.div(one, cos2))
        }
        FnTag::Exp  => Some(pool.func(FnTag::Exp, vec![u])),
        FnTag::Log  => {
            let one = pool.one;
            Some(pool.div(one, u))
        }
        FnTag::Sqrt => {
            let two = pool.small_int(2);
            let sqrt_u = pool.func(FnTag::Sqrt, vec![u]);
            let denom = pool.mul(vec![two, sqrt_u]);
            let one = pool.one;
            Some(pool.div(one, denom))
        }
        FnTag::Asin => {
            let two_int = pool.small_int(2);
            let u2 = pool.pow(u, two_int);
            let neg_u2 = pool.neg(u2);
            let one = pool.one;
            let one_minus_u2 = pool.add(vec![one, neg_u2]);
            let sqrt = pool.func(FnTag::Sqrt, vec![one_minus_u2]);
            Some(pool.div(one, sqrt))
        }
        FnTag::Acos => {
            let two_int = pool.small_int(2);
            let u2 = pool.pow(u, two_int);
            let neg_u2 = pool.neg(u2);
            let one = pool.one;
            let one_minus_u2 = pool.add(vec![one, neg_u2]);
            let sqrt = pool.func(FnTag::Sqrt, vec![one_minus_u2]);
            let pos = pool.div(one, sqrt);
            Some(pool.neg(pos))
        }
        FnTag::Atan => {
            let two_int = pool.small_int(2);
            let u2 = pool.pow(u, two_int);
            let one = pool.one;
            let denom = pool.add(vec![one, u2]);
            Some(pool.div(one, denom))
        }
        FnTag::Abs  => None, // undefined at 0; Phase 1 placeholder
        FnTag::Custom(_) => None,
    }
}
