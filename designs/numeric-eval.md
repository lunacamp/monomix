# Numeric Evaluation — System Design

**Component:** `monomix-kernel::evalnum`
**Status:** Design phase
**Date:** 2026-05-04 (split from `designs/substitution-numeric-eval.md`, originally dated 2026-05-03)
**References:** SCOPE.md §1.8, §1.3, §0.3, §0.4, §0.7; ADR-0001; ADR-0002; `designs/expression-dag.md`; `designs/parser.md`; `designs/simplifier.md`; `designs/polynomial-ops.md`; `designs/equation-solving.md`; `designs/differentiation.md`; `designs/substitution.md`

---

## 1. Requirements

### 1.1 Functional requirements

The `evalnum` engine walks an expression and produces an `f64` numeric value. Every leaf must reduce to either a numeric atom (`SmallInt`, `BigInt`, `Rational`, `Float`) or a symbol whose binding eventually reduces to one. Built-in functions (`sin`, `cos`, `exp`, `log`, `sqrt`, `abs`, `asin`, `acos`, `atan`) are evaluated against `libm`.

The required surface from SCOPE.md §1.8:

- `evaluate_numeric(expr)` returning a Python `float`. Symbols without bindings raise `EvalError` (a subclass of `monomix.MonomixError` per SCOPE.md §0.4).
- This is the *only* path that mixes symbolic and floating-point representations (SCOPE.md §1.8 verbatim). Every other Phase 1 module operates either fully symbolically or, in the case of `Float` literals, treats the float as an opaque atom (`designs/simplifier.md` §3.2).

The numeric evaluator is consumed by:

- The **simplifier's property-based test harness** (`designs/simplifier.md` §6.2 — `evaluate_numeric(simplify(e), b) == evaluate_numeric(e, b)` is the load-bearing invariant for "simplify preserves meaning").
- The **REPL's `evaluate_numeric` builtin** (SCOPE.md §1.9 command list).
- The **solver's fuzz harness** (`designs/equation-solving.md` §6.4 — the quadratic-round-trip target evaluates roots numerically).
- Plotting and visualisation paths (Phase 2; out of scope here).

### 1.2 Non-functional requirements

- **Bounded time.** `evalnum` is a single top-down fold over the input DAG, linear in the number of distinct nodes. No fixed-point loop, no recursive simplifier callback.
- **No panics.** Errors are returned as `KernelError::EvalError` variants; they surface in Python as `monomix.EvalError` (SCOPE.md §0.4 — the SCOPE-mandated name).
- **Determinism.** Two `evalnum` calls produce bit-identical `f64` outputs (modulo libm's documented determinism — see §3.3.3).
- **DAG-safe.** The walk uses the visited-set discipline from `designs/expression-dag.md` §3.6. A subexpression shared between two parents is evaluated once and the result is memoised. Combined with binding lookup, this is what makes evaluating the simplifier's typical heavily-shared output tractable.

### 1.3 Constraints

- **`f64`-only.** SCOPE.md §1.8 specifies "Python `float`" as the output type. Arbitrary-precision numeric evaluation (`mpf` from MPFR, or Decimal-based) is a Phase 3+ candidate; Phase 1 does not surface it.
- **No partial evaluation.** `evalnum` is all-or-nothing — if any subterm fails to reduce to `f64`, the call returns `EvalError`. There is no "evaluate as far as possible and return a half-symbolic result" mode. The simplifier's symbolic folding (`designs/simplifier.md` §3.2) plus an explicit `substitute` pass (`designs/substitution.md` §3.1) are the user's tools for partial evaluation.
- **`evalnum` is not the simplifier.** It does not fold `0 + x = x` symbolically before numeric eval; it dispatches on `ExprNode` variants directly. If a subterm is `Add([0, x])` and `x` evaluates to `3`, `evalnum` returns `0 + 3 = 3`, not via the simplifier. The simplifier is the right tool for symbolic cleanup *before* `evalnum`.
- **No NaN propagation.** Every operation that would produce NaN raises `EvalError` instead. NaN-flavored f64 values never escape the engine in Phase 1.

### 1.4 What this component is **not**

To pin scope precisely:

- It is **not the parser.** The parser handles `evaluate_numeric(expr)` syntax and lowers it (`designs/parser.md` §3.3). The engine receives an already-parsed `ExprId` plus a binding map.
- It is **not the simplifier.** It neither rewrites nor cancels; it evaluates. An `evalnum` result is just a number.
- It is **not the Session.** Bindings live on `Session` (Python side, SCOPE.md §1.3). The engine is passed an explicit `Bindings` view; the Session is the policy layer that decides which bindings to pass.
- It is **not the differentiator.** It cannot evaluate `df(f, x)` symbolically; if `evalnum` encounters a `Fn(Custom("df"), ...)` it raises `EvalError` unless the derivative has already been computed and stored (the Session's job, per `designs/differentiation.md`).
- It does **not** track tolerances, error bounds, or interval arithmetic. Output is plain IEEE-754 `f64`; the user owns any error analysis they need.
- It is **not the substitute engine.** The two engines compose but neither subsumes the other: `evalnum` consults bindings during the walk rather than pre-substituting (§4.1).

---

## 2. High-Level Design

### 2.1 Public API

```rust
//! crates/monomix-kernel/src/evalnum/mod.rs

/// Numerically evaluate `root` against `bindings` and return an f64. Every leaf must
/// reduce to a numeric value; unbound symbols raise `EvalError::UnboundSymbol`.
///
/// Bindings are consulted *during* the walk — the engine does not pre-substitute
/// the entire expression and then evaluate. This is more efficient (no intermediate
/// ExprIds allocated) and lets the engine short-circuit on the first error.
///
/// Errors:
/// - `KernelError::EvalError(UnboundSymbol(name))` — a Symbol with no binding.
/// - `KernelError::EvalError(DivisionByZero)` — a Div whose denominator evaluates to 0.
/// - `KernelError::EvalError(LogOfNonPositive(x))` — log(x) for x ≤ 0.
/// - `KernelError::EvalError(SqrtOfNegative(x))` — sqrt(x) for x < 0.
/// - `KernelError::EvalError(DomainError { fn_name, arg })` — asin/acos out of [-1,1].
/// - `KernelError::EvalError(UnsupportedFn(tag))` — a `Fn(Custom(name), …)` with no
///   numeric implementation (e.g. user-defined Phase 2 plugin function not registered
///   for numeric eval).
pub fn evaluate_numeric(
    pool: &ExprPool,
    bindings: &Bindings,
    root: ExprId,
) -> Result<f64, KernelError>;

/// Variant that returns `(value, NaN_flag)` — distinguishes "evaluated to NaN" (which
/// is a valid f64 result, e.g. from 0/0 if division-by-zero handling were lenient)
/// from "evaluation succeeded with a finite value". Phase 1 does not actually allow
/// arithmetic NaN propagation — every NaN-producing operation raises EvalError —
/// but this variant is the hook the test harness uses to assert that.
pub fn evaluate_numeric_strict(
    pool: &ExprPool,
    bindings: &Bindings,
    root: ExprId,
) -> Result<f64, KernelError>;

/// Read-only binding map passed to the evaluator. Owned by `Session` (per SCOPE.md
/// §1.3); the engine borrows it immutably so the same `Session` can run multiple
/// evals concurrently if Phase 2's per-request session split (`designs/parser.md` §5.2)
/// lands.
pub struct Bindings<'a> {
    /// Map from Symbol → ExprId. Resolved one level at a time by the Session before
    /// reaching this engine; cycles are pre-detected at the Session layer
    /// (`designs/substitution.md` §3.6).
    pub map: &'a FxHashMap<Symbol, ExprId>,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum EvalError {
    #[error("symbol {0:?} has no binding")]
    UnboundSymbol(Symbol),
    #[error("division by zero")]
    DivisionByZero { span: Option<Span> },
    #[error("log({0}) is undefined for non-positive argument")]
    LogOfNonPositive(f64),
    #[error("sqrt({0}) is undefined for negative argument")]
    SqrtOfNegative(f64),
    #[error("{fn_name}({arg}) is out of domain")]
    DomainError { fn_name: &'static str, arg: f64 },
    #[error("function {0:?} has no numeric implementation")]
    UnsupportedFn(FnTag),
    #[error("integer overflow converting {0} to f64")]
    IntegerOverflow(BigInt),
}
```

The `Bindings` type wraps a `FxHashMap<Symbol, ExprId>` borrowed from the Session rather than owning it. This is deliberate: `Session` already owns the binding table (SCOPE.md §1.3), and the evaluator only reads — there is no need for a copy. The `'a` lifetime ties the `Bindings` view to the call.

`EvalError` is flattened into `KernelError::EvalError(_)` at the boundary so callers pattern-match without threading through an extra `Result`. This matches the convention from `designs/equation-solving.md` §3.9, `designs/simplifier.md` §3.9, and `designs/substitution.md` §3.8.

### 2.2 Component diagram

```
              ExprId (root) + Bindings
                       │
                       ▼
            ┌─────────────────────┐
            │ evalnum/            │
            │   walk.rs           │
            │  (fold + dispatch   │
            │   on ExprNode)      │
            └──────────┬──────────┘
                       │
                       │ binding lookup → recurse
                       │ atom → numeric coercion
                       │ Fn → funcs::dispatch
                       ▼
            ┌──────────────────────┐
            │ evalnum/funcs.rs     │
            │  (sin/cos/exp/log/   │
            │   sqrt/abs/asin/...) │
            └──────────┬───────────┘
                       │
                       ▼
                       f64
```

The mirror pipeline for substitution is in `designs/substitution.md` §2.2. Both walks consume a binding map but the substitute walk writes back into the pool whereas evalnum produces a scalar.

### 2.3 Module layout

```
crates/monomix-kernel/src/evalnum/
├── mod.rs              — public API, EvalError, Bindings type, re-exports
├── walk.rs             — fold-based evaluator; ExprNode dispatch
├── funcs.rs            — built-in numeric function table (sin, cos, …)
├── coerce.rs           — ExprNode → f64 atom coercion (with overflow checks)
└── tests.rs
```

This module does not cross-import `substitute/`. The shared `fold` primitive lives in `expression-dag` (`designs/expression-dag.md` §3.6). Putting them in a single combined `eval/` directory was considered and rejected — the operations have nothing in common at the implementation level (substitute writes to the pool, evalnum doesn't), and the cache shapes are different enough that a shared file would be a worse organising principle than the two design docs that cross-reference each other.

### 2.4 Algorithm choices at a glance

| Operation | Algorithm | Complexity | Notes |
|-----------|-----------|------------|-------|
| `evaluate_numeric` | `fold` with f64 accumulator; ExprNode dispatch; `funcs::dispatch` for `Fn(tag, args)` | O(distinct nodes) | No allocation; no intermediate ExprIds; failure short-circuits the walk |
| Numeric atom coercion | `SmallInt → as f64`; `BigInt → to_f64`; `Rational → num/den (with f64 division)`; `Float → into_inner` | O(1) per atom | Overflow check on `BigInt` past 2^53 raises `IntegerOverflow` |
| Symbol lookup | `bindings.map.get(&Symbol(...))` then recurse on the bound `ExprId` | O(1) per Symbol | One memoisation entry per Symbol resolved — the visited HashMap covers it |

The conspicuous absence is a partial-evaluation mode ("evaluate what you can, leave the rest symbolic"). That is the Phase 2 `simplify_with_bindings` candidate (§5.4) and lives one layer up in the simplifier; this engine is strictly all-or-nothing.

### 2.5 Single-pass, no fixed-point

`evalnum` has no fixed-point loop. Each call performs:

1. `fold` over the DAG with an f64 accumulator. For each node, dispatch on `ExprNode` variant: atom → coerce; binary → recurse + apply; `Fn(tag, args)` → recurse + dispatch through `funcs::dispatch`.
2. Short-circuit on the first error (the fold's accumulator is `Result<f64, _>`).
3. Return the f64 (or the error).

---

## 3. Deep Dive

### 3.1 Numeric evaluator (`evalnum/walk.rs`)

The evaluator is a single fold over the DAG returning `Result<f64, EvalError>`.

```rust
pub fn evaluate_numeric(
    pool: &ExprPool,
    bindings: &Bindings,
    root: ExprId,
) -> Result<f64, KernelError> {
    let mut visited: FxHashMap<ExprId, f64> = FxHashMap::default();
    eval_node(pool, bindings, root, &mut visited)
}

fn eval_node(
    pool: &ExprPool,
    bindings: &Bindings,
    id: ExprId,
    visited: &mut FxHashMap<ExprId, f64>,
) -> Result<f64, KernelError> {
    if let Some(&v) = visited.get(&id) { return Ok(v); }

    let v = match pool.get(id) {
        // --- Numeric atoms: direct coercion ---
        ExprNode::SmallInt(k)        => *k as f64,
        ExprNode::BigInt(b)          => coerce::bigint_to_f64(b)?,
        ExprNode::Rational(p)        => coerce::rational_to_f64(&p.0, &p.1)?,
        ExprNode::Float(f)           => f.into_inner(),

        // --- Symbols: binding lookup ---
        ExprNode::Symbol(s) => match bindings.map.get(&Symbol(*s)) {
            Some(&bound) => eval_node(pool, bindings, bound, visited)?,
            None         => return Err(KernelError::Eval(EvalError::UnboundSymbol(Symbol(*s)))),
        },

        // --- Strings cannot be numerically evaluated ---
        ExprNode::String(_) => return Err(KernelError::Eval(EvalError::UnsupportedFn(
            FnTag::Custom(InternedStr::SENTINEL_STRING_LITERAL)
        ))),

        // --- Composites: recurse + apply ---
        ExprNode::Add(children) => {
            let mut acc = 0.0;
            for &c in children.iter() { acc += eval_node(pool, bindings, c, visited)?; }
            acc
        }
        ExprNode::Mul(children) => {
            let mut acc = 1.0;
            for &c in children.iter() { acc *= eval_node(pool, bindings, c, visited)?; }
            acc
        }
        ExprNode::Pow(b, e) => {
            let bv = eval_node(pool, bindings, *b, visited)?;
            let ev = eval_node(pool, bindings, *e, visited)?;
            powf_with_domain_checks(bv, ev)?
        }
        ExprNode::Neg(x)    => -eval_node(pool, bindings, *x, visited)?,
        ExprNode::Div(n, d) => {
            let nv = eval_node(pool, bindings, *n, visited)?;
            let dv = eval_node(pool, bindings, *d, visited)?;
            if dv == 0.0 { return Err(KernelError::Eval(EvalError::DivisionByZero { span: None })); }
            nv / dv
        }
        ExprNode::Eq(_, _)  => return Err(KernelError::Eval(EvalError::UnsupportedFn(
            FnTag::Custom(InternedStr::SENTINEL_EQ_NODE)
        ))),
        ExprNode::Fn(tag, args) => funcs::dispatch(pool, bindings, *tag, args, visited)?,
        ExprNode::List(_)   => return Err(KernelError::Eval(EvalError::UnsupportedFn(
            FnTag::Custom(InternedStr::SENTINEL_LIST_NODE)
        ))),
    };
    visited.insert(id, v);
    Ok(v)
}
```

**Why memoize `Symbol` resolutions.** A symbol that appears `n` times in the expression resolves once via the binding lookup, not `n` times. For a simplifier output where `x` appears in many places after substitution (e.g. `(x+1)^10` expanded), this is the difference between O(n) and O(n × depth-of-x's-binding-chain) numeric work.

**Why `String`, `Eq`, and `List` are explicit errors.** These exprnodes have no meaningful f64 representation. The user almost certainly intends to evaluate the *content* of an Eq (its LHS or RHS) or a List (each element), not the structural node itself. Raising `UnsupportedFn` with a dedicated sentinel is the clearest signal that the user needs to extract the numeric portion first. (The "sentinel" InternedStr constants are kernel-reserved names that the Python boundary translates to readable strings — see §3.6.)

**Why `Add` and `Mul` use Σ/Π loops, not pairwise reduce.** N-ary nodes are stored flattened (`designs/expression-dag.md` §2.1 — Add/Mul are `Box<[ExprId]>`). The loops match the storage shape and do not introduce associativity-driven precision quirks (other than the standard left-to-right f64 summation order, which is documented in §3.3.3).

### 3.2 Atom coercion (`evalnum/coerce.rs`)

```rust
pub fn bigint_to_f64(b: &BigInt) -> Result<f64, KernelError> {
    use num_traits::ToPrimitive;
    match b.to_f64() {
        Some(f) if f.is_finite() => Ok(f),
        _ => Err(KernelError::Eval(EvalError::IntegerOverflow(b.clone()))),
    }
}

pub fn rational_to_f64(num: &BigInt, den: &BigInt) -> Result<f64, KernelError> {
    let n = bigint_to_f64(num)?;
    let d = bigint_to_f64(den)?;
    if d == 0.0 {
        return Err(KernelError::Eval(EvalError::DivisionByZero { span: None }));
    }
    Ok(n / d)
}
```

**Overflow on BigInt → f64.** `num_bigint::BigInt::to_f64` returns `None` for values that don't fit in `f64`'s ~10^308 range, and `Some(f64::INFINITY)` for values past the f64 max-finite. Both cases are surfaced as `IntegerOverflow` rather than silently returning infinity — once an evaluation is contaminated with `inf`, every downstream arithmetic produces NaN-or-inf and the eventual error message is much further from the root cause. The eager check at coercion time is the user-friendly choice.

**Rational precision.** Computing `num/den` as `f64` loses precision when both numerator and denominator are large but their ratio is small. The alternative is to use `BigRational::to_f64` directly (which the `num-rational` crate implements via binary long division to 53 bits of mantissa). Phase 1 picks the simpler `n/d` path because the precision loss is bounded by ~1 ulp for typical inputs, and the property-based tests (§6.2) compare against `Rational::to_f64` to detect regressions beyond this tolerance.

A future Phase 2 swap to `BigRational::to_f64` is one function-body change (§5.1).

### 3.3 `pow` — the four cases

```rust
fn powf_with_domain_checks(base: f64, exp: f64) -> Result<f64, KernelError> {
    // 1. Negative base, non-integer exponent: NaN territory in real arithmetic.
    //    Phase 1 raises DomainError rather than returning NaN.
    if base < 0.0 && exp.fract() != 0.0 {
        return Err(KernelError::Eval(EvalError::DomainError {
            fn_name: "pow", arg: base,
        }));
    }
    // 2. Zero base, negative or zero exponent: undefined.
    if base == 0.0 && exp <= 0.0 {
        if exp == 0.0 {
            // 0^0 — REDUCE returns 1 (the conventional polynomial choice).
            return Ok(1.0);
        }
        return Err(KernelError::Eval(EvalError::DivisionByZero { span: None }));
    }
    // 3. Standard case.
    let r = base.powf(exp);
    if r.is_nan() {
        return Err(KernelError::Eval(EvalError::DomainError {
            fn_name: "pow", arg: base,
        }));
    }
    Ok(r)
}
```

**`0^0 = 1`.** The polynomial convention is what Phase 1 ships, matching REDUCE's `expt(0, 0) = 1` ([alg/aritho.red](../legacy/reduce-algebra-code-r7357-trunk/packages/alg/aritho.red)). Floating-point IEEE-754 also defines `pow(0.0, 0.0) = 1.0`, so we are consistent across both paths. The mathematicians who insist on `0^0 = indeterminate` can call `evaluate_numeric_strict` and get the same `1.0` answer — Phase 1 does not have an "indeterminate" branch.

**Negative base with non-integer exponent.** `(−2)^0.5` is complex (`i√2`) and Phase 1 does not represent complex numbers (SCOPE.md §3.1 — Phase 3+). Returning `NaN` silently would produce useless error messages downstream; raising `DomainError` at the exact site is the right call. The check `exp.fract() != 0.0` correctly identifies integer exponents stored as floats (`2.0`, `3.0`) and lets them through to `powf`, which evaluates `(−2)^2 = 4`.

### 3.3.3 Built-in function dispatch (`evalnum/funcs.rs`)

```rust
pub fn dispatch(
    pool: &ExprPool,
    bindings: &Bindings,
    tag: FnTag,
    args: &[ExprId],
    visited: &mut FxHashMap<ExprId, f64>,
) -> Result<f64, KernelError> {
    let evaluated: Vec<f64> = args.iter()
        .map(|&a| eval_node(pool, bindings, a, visited))
        .collect::<Result<_, _>>()?;
    let r = match (tag, evaluated.as_slice()) {
        (FnTag::Sin,  [x])  => x.sin(),
        (FnTag::Cos,  [x])  => x.cos(),
        (FnTag::Tan,  [x])  => x.tan(),
        (FnTag::Exp,  [x])  => x.exp(),
        (FnTag::Log,  [x])  => check_log(*x)?,
        (FnTag::Sqrt, [x])  => check_sqrt(*x)?,
        (FnTag::Abs,  [x])  => x.abs(),
        (FnTag::Asin, [x])  => check_inverse_trig("asin", *x, x.asin())?,
        (FnTag::Acos, [x])  => check_inverse_trig("acos", *x, x.acos())?,
        (FnTag::Atan, [x])  => x.atan(),
        (other, _) => return Err(KernelError::Eval(EvalError::UnsupportedFn(other))),
    };
    Ok(r)
}

fn check_log(x: f64) -> Result<f64, KernelError> {
    if x <= 0.0 {
        Err(KernelError::Eval(EvalError::LogOfNonPositive(x)))
    } else {
        Ok(x.ln())
    }
}

fn check_sqrt(x: f64) -> Result<f64, KernelError> {
    if x < 0.0 {
        Err(KernelError::Eval(EvalError::SqrtOfNegative(x)))
    } else {
        Ok(x.sqrt())
    }
}

fn check_inverse_trig(name: &'static str, arg: f64, result: f64) -> Result<f64, KernelError> {
    if result.is_nan() {
        Err(KernelError::Eval(EvalError::DomainError { fn_name: name, arg }))
    } else {
        Ok(result)
    }
}
```

**The function table is closed in Phase 1.** Custom functions (`FnTag::Custom(name)`) that arrive in Phase 1.10's plugin system either register an `evalnum_handler` callback or accept that `evaluate_numeric` will raise `UnsupportedFn`. The plugin contract is documented in `designs/plugin-system.md` (Phase 1.10 design TBD); for Phase 1, only the kernel built-ins above are evaluable.

**libm determinism.** Every `f64::sin`, `cos`, `tan`, etc. routes through Rust's `std::f64::sin` and friends, which are documented to be deterministic for a given input on a given target (Rust references the platform's libm). Cross-target bit-identical results are *not* guaranteed (x86-64 vs. ARM may differ in the last bit on boundary inputs). The property-based tests (§6.2) account for this by allowing `abs_diff <= 4 * f64::EPSILON * max(|expected|, 1.0)` rather than bit-exact equality.

**Why not `f64::ln_1p` for `log(1 + x)`?** That is a common precision improvement, but the user passes `log(1 + x)` as `Log([Add([1, x])])` — by the time `funcs::dispatch` sees the argument, it is already a single `f64`. Recovering the structural form would require pattern matching back through the ExprNode, which is the simplifier's job, not ours. A future "numerically-stable rewrite pass" (§5.2 candidate) is the right home.

### 3.4 The `Bindings` type and its lifetime story

```rust
pub struct Bindings<'a> {
    pub map: &'a FxHashMap<Symbol, ExprId>,
}

impl<'a> Bindings<'a> {
    /// Construct from a Session's binding table.
    pub fn from_session(map: &'a FxHashMap<Symbol, ExprId>) -> Self {
        Bindings { map }
    }

    /// Construct from an explicit Python dict (the `subs({x: 5})` path).
    pub fn from_dict(map: &'a FxHashMap<Symbol, ExprId>) -> Self {
        Bindings { map }
    }

    /// Empty binding set — used by the test harness for "must be fully concrete" checks.
    pub fn empty() -> Self {
        // Static-lifetime trick: a thread-local empty hashmap.
        thread_local! { static EMPTY: FxHashMap<Symbol, ExprId> = FxHashMap::default(); }
        EMPTY.with(|m| Bindings { map: unsafe { &*(m as *const _) } })
    }
}
```

The `Bindings` type is a thin borrow-newtype rather than a full owning structure because:

1. The Session's binding table already exists (SCOPE.md §1.3). Copying it on every evalnum call would be wasteful when expressions can be deeply nested.
2. The PyO3 boundary builds a fresh `FxHashMap` from a Python dict per call (§3.6); that map is the owner, and `Bindings` borrows it for the call's duration.
3. Phase 2's per-request session split (`designs/parser.md` §5.2) needs concurrent reads of the same binding map from multiple threads — the borrow-only design makes that trivial (immutable shared reference), where an owning structure would force a clone or an `Arc`.

The `unsafe` in `Bindings::empty()` is the standard `thread_local!` lifetime cast and is sound because the lifetime of the returned reference is tied to the function's return value, which is always immediately consumed by the evaluator.

### 3.5 Composition with substitution and the Session

The Session decides, for each evalnum call, whether to:

- pass its full binding table directly (the typical REPL path); or
- compose with `Session.resolve(...)` first, materialising bindings into the expression before evalnum runs.

The two paths are equivalent for healthy inputs but produce different error messages on cycles:

- **Pass full table.** The cycle-detection check happens lazily as the binding is dereferenced; the error site is in the middle of the f64 walk and the diagnostic is "binding chain exceeded N levels".
- **Resolve first.** The cycle is detected up front by `designs/substitution.md` §3.6's resolver, before any numeric work begins; the diagnostic names the offending symbol.

The REPL prefers the second path for clearer messages. Library callers invoking `evaluate_numeric(expr, bindings_dict)` directly get the first path. Both are supported.

### 3.6 PyO3 boundary

The numeric evaluator is exposed to Python via `Session.evaluate_numeric(...)` and `Expr.evaluate_numeric(dict=None)`.

```rust
#[pyfunction]
fn evaluate_numeric(py: Python<'_>, session: &PySession, expr: &PyExpr, bindings: Option<&PyDict>)
    -> PyResult<f64>
{
    let pool_handle = expr.pool.clone();
    let raw_map = match bindings {
        Some(dict) => py_dict_to_binding_map(py, &pool_handle, dict)?,
        None       => session.bindings_table.read().clone(),
    };
    let id = expr.id;
    let subtree_size = pool_handle.read().subtree_size(id);
    let pool = pool_handle.read();
    let bindings = monomix_kernel::evalnum::Bindings::from_session(&raw_map);
    let value = if subtree_size > 500 {
        py.allow_threads(|| monomix_kernel::evalnum::evaluate_numeric(&pool, &bindings, id))?
    } else {
        monomix_kernel::evalnum::evaluate_numeric(&pool, &bindings, id)?
    };
    Ok(value)
}
```

**GIL release threshold.** Same `subtree_size > 500` cutoff as the simplifier (`designs/simplifier.md` §3.8), polynomial engine (`designs/polynomial-ops.md` §3.8), substitute (`designs/substitution.md` §3.7), and solver (`designs/equation-solving.md` §3.8). Uniform policy across all kernel surface ops keeps the boundary-overhead profile predictable.

**Read lock for evalnum.** `evaluate_numeric` does not allocate new `ExprId`s, so it takes a *read* lock on the pool rather than the substitute walk's write lock. This means concurrent `evaluate_numeric` calls from multiple Python threads do not serialize on the pool — important for the property-based-test path which runs many concurrent random-binding evaluations.

**Sentinel translation.** The `UnsupportedFn(InternedStr::SENTINEL_*)` errors emitted by `walk.rs` (§3.1) carry kernel-internal interned strings. The boundary translates them to user-friendly Python strings:

```rust
fn render_unsupported_fn(tag: FnTag) -> String {
    match tag {
        FnTag::Custom(s) if s == InternedStr::SENTINEL_EQ_NODE       => "Eq".into(),
        FnTag::Custom(s) if s == InternedStr::SENTINEL_LIST_NODE     => "List".into(),
        FnTag::Custom(s) if s == InternedStr::SENTINEL_STRING_LITERAL => "String".into(),
        FnTag::Custom(s) => s.as_str().into(),
        other            => format!("{other:?}"),
    }
}
```

So the user sees `EvalError: cannot numerically evaluate Eq node — extract LHS or RHS first` rather than `EvalError: UnsupportedFn(Custom(InternedStr(0xff_ff_ff_fe)))`.

### 3.7 Error handling

| Error | Source | Handling |
|-------|--------|----------|
| `EvalError::UnboundSymbol(sym)` | Walk hits Symbol with no binding | Return; the Symbol's name is included for diagnostics |
| `EvalError::DivisionByZero` | Div with denominator evaluating to 0.0; or Rational with den == 0 (which the pool should never construct) | Return; `span: None` because the source span is at the offending Div, not at evalnum |
| `EvalError::LogOfNonPositive(x)` | `Fn(Log, [arg])` with arg ≤ 0 | Return; `x` is the offending value |
| `EvalError::SqrtOfNegative(x)` | `Fn(Sqrt, [arg])` or `Pow(b, 0.5)` with arg/b < 0 | Return; `x` is the offending value |
| `EvalError::DomainError { fn_name, arg }` | asin/acos out of [-1,1]; pow with negative base + non-integer exponent | Return; both `fn_name` and `arg` are reported |
| `EvalError::IntegerOverflow(b)` | BigInt → f64 conversion exceeds f64 max-finite | Return; the BigInt value is included for diagnostics |
| `EvalError::UnsupportedFn(tag)` | A Fn tag with no numeric implementation; or non-numeric ExprNode (Eq, List, String) | Return; sentinel tags are translated at the PyO3 boundary |

The engine never panics. Internal invariant violations are caught by `debug_assert!` in debug builds and become a `KernelError::Internal` sentinel in release builds, which the boundary maps to a generic "internal error, please report" message.

The `Span` field on `EvalError::DivisionByZero` is populated when the originating `Div` ExprId carries a span via the parser's `SpanMap` (`designs/parser.md` §3.5). For the other variants, no span is attached because the failure is determined by a runtime value (the f64 result of a sub-evaluation) rather than a syntactic location.

---

## 4. Trade-off Analysis

### 4.1 Numeric eval as fold vs. as substitute-then-fold

**Chosen: direct fold — bindings consulted during the walk.**

A simpler implementation strategy is "first call `substitute_many` with the bindings (`designs/substitution.md` §3.2), then evaluate the now-fully-numeric expression". This avoids the binding lookup logic in evalnum entirely.

| Approach | Allocations | First-error short-circuit | Pool churn |
|----------|-------------|---------------------------|------------|
| Direct fold (chosen) | One f64 per visited node + visited HashMap | Yes — fail at the first unbound symbol | None |
| Substitute then fold (rejected) | One ExprId per substituted node + the eval allocations | No — full substitution must complete first | Significant — every numeric eval interns a copy of the substituted tree |

The direct fold also avoids a subtle correctness issue: `substitute({x: 1/0}, x + y)` would silently produce a Div(1, 0) node that subsequent eval flags. The direct fold sees the `1/0` ExprId as a binding *value*, recurses into it, and raises `DivisionByZero` at the actual numeric site rather than at the eventual top-level operation. The error message is more useful as a result.

### 4.2 `Bindings` borrow vs. owning

**Chosen: borrow (`&'a FxHashMap<Symbol, ExprId>`).**

The `Bindings` type wraps a borrow rather than owning the map. The owning alternative would be `Bindings(FxHashMap<Symbol, ExprId>)` cloned per call.

| Approach | Per-call cost | Concurrency | Lifetime ergonomics |
|----------|---------------|-------------|---------------------|
| Borrow (chosen) | None — pointer copy | Multiple readers safe | `'a` lifetime threading needed |
| Owning | One HashMap clone per call | Trivially safe | No lifetimes — easier |

For Phase 1 single-threaded sessions, the difference is negligible (~1 µs per clone). For Phase 2's per-request session split (`designs/parser.md` §5.2), where multiple threads concurrently evaluate against the same binding table, the borrow design lets all of them share the read without contention. Choosing the borrow now makes Phase 2 additive rather than a refactor.

### 4.3 NaN-as-error vs. NaN-as-value

**Chosen: NaN-producing operations raise `EvalError`; NaN never escapes the engine.**

The IEEE-754 alternative is to let `0/0`, `log(-1)`, `(-2)^0.5`, etc. propagate as `f64::NAN` and let the user check `result.isnan()` after the fact.

| Approach | Failure visibility | Interop with Python `float` | Composability |
|----------|-------------------|-----------------------------|---------------|
| Error-on-NaN (chosen) | At the failure site | `float('nan')` only by user request | Errors compose with Result; short-circuit |
| Propagate NaN | At the user's check site, possibly far from cause | Direct `float('nan')` value | NaN poisons every subsequent op |

The error-on-NaN choice surfaces problems at their actual source (the offending `log(-1)`) rather than three steps later when the user finally checks. For users who genuinely want NaN propagation (rare; almost always a programming error in CAS workflows), the `evaluate_numeric_strict` variant is the hook to add a `nan_policy: Policy` parameter in Phase 2 without breaking the Phase 1 default contract.

---

## 5. Scale, Limits, and Future Work

### 5.1 Phase 2: BigRational precision refinement

Replace `bigint_to_f64(num) / bigint_to_f64(den)` in `coerce::rational_to_f64` with `BigRational::to_f64`, which performs binary long division to 53 bits of mantissa and avoids the precision loss when both `num` and `den` are large (§3.2). One function-body change, no API surface impact.

### 5.2 Phase 2: Numerically stable rewrite pass

A simplifier extension (a rule set in `designs/simplifier.md` §3.6) that recognizes:

- `log(1 + x)` → `ln_1p(x)` for small `x`
- `exp(x) − 1` → `expm1(x)` for small `x`
- `sqrt(1 + x²) − 1` → hypot/2-style rewrites
- `1 - cos(x)` → `2 * sin(x/2)^2` for small `x`

These rules fire before evalnum sees the expression, so evalnum benefits passively. Lives in the simplifier, not in evalnum, because the rewrites are syntactic.

### 5.3 Phase 2: MPFR backend behind an `arb` feature flag

Users who need >53 bits of precision opt in via `monomix[arb]`. The `evaluate_numeric` API gains a `precision: Precision` parameter; the f64 path remains the default. This is a feature-flagged dependency add (`rug` or direct `gmp-mpfr-sys`), not a redesign of the walk. The dispatch in `walk.rs` becomes generic over the numeric type via a `NumericBackend` trait.

### 5.4 Phase 2: Partial numeric evaluation

A `simplify_with_bindings(expr, bindings)` API that performs substitution + simplify + *partial* numeric evaluation — fold every fully-numeric subtree into a number, leave symbolic subtrees untouched. This is the right home for "evaluate as far as you can" use cases. Lives in the simplifier (it composes substitute + simplify + a numeric folding rule), not in this engine. Phase 2 deliverable.

### 5.5 Phase 1.10: Plugin functions in evalnum

Phase 1.10's plugin system (TBD in its own design doc) needs a story for plugin-provided functions in evalnum. The current design's `UnsupportedFn(tag)` error is the honest Phase 1 answer; Phase 1.10's contract should add an `evalnum_handler: fn(args: &[f64]) -> Result<f64, EvalError>` callback that the dispatch table consults for `FnTag::Custom`. This is a contract addition, not a redesign of the eval walker.

### 5.6 Performance characteristics

For Phase 1, expected complexity:

| Input | Time | Allocations |
|-------|------|-------------|
| `evaluate_numeric(e, full_bindings)`, `e` is small | O(distinct nodes) ~ 0.1 ms | Visited HashMap |
| `evaluate_numeric(e, full_bindings)`, `e` is large + heavily shared | O(distinct nodes) ~ 1 ms | Visited HashMap (load-bearing — without it, blow up) |
| `subs + evaluate_numeric` pipeline (small expr) | <200 µs combined (see `designs/substitution.md` §5.4) | Substitute cache + visited HashMap |

The dominant term in evalnum is the visited-HashMap lookups; for the simplifier-test property path which calls evalnum ~1000 times per test on small expressions, the per-call overhead is the regression target. The §6.3 benchmarks pin this.

---

## 6. Testing Strategy

### 6.1 Unit tests (`cargo test`)

**Atoms:**

- `evalnum(5)` ⟹ `5.0`.
- `evalnum(Rational(1, 2))` ⟹ `0.5`.
- `evalnum(Float(3.14))` ⟹ `3.14`.
- `evalnum(BigInt(huge))` for `huge > 2^1023` ⟹ `Err(IntegerOverflow)`.
- `evalnum(Symbol("x"))` with no binding ⟹ `Err(UnboundSymbol(x))`.
- `evalnum(Symbol("x"))` with `{x: 5}` ⟹ `5.0`.

**Composites:**

- `evalnum(2 + 3)` ⟹ `5.0`.
- `evalnum(2 * 3 + 4)` ⟹ `10.0`.
- `evalnum(2^3)` ⟹ `8.0`.
- `evalnum(1/0)` ⟹ `Err(DivisionByZero)`.
- `evalnum((-1)^0.5)` ⟹ `Err(DomainError { fn_name: "pow", arg: -1.0 })`.
- `evalnum(0^0)` ⟹ `1.0`.
- `evalnum(0^(-1))` ⟹ `Err(DivisionByZero)`.

**Built-in functions:**

- `evalnum(sin(0))` ⟹ `0.0`.
- `evalnum(cos(pi))` with `{pi: π_value}` ⟹ `-1.0` (within tolerance).
- `evalnum(log(1))` ⟹ `0.0`.
- `evalnum(log(0))` ⟹ `Err(LogOfNonPositive(0.0))`.
- `evalnum(log(-1))` ⟹ `Err(LogOfNonPositive(-1.0))`.
- `evalnum(sqrt(4))` ⟹ `2.0`.
- `evalnum(sqrt(-1))` ⟹ `Err(SqrtOfNegative(-1.0))`.
- `evalnum(asin(2))` ⟹ `Err(DomainError { fn_name: "asin", arg: 2.0 })`.
- `evalnum(abs(-3))` ⟹ `3.0`.

**Non-numeric nodes:**

- `evalnum(Eq(x, 5))` ⟹ `Err(UnsupportedFn)` mapping to user-visible "Eq".
- `evalnum(List([1, 2]))` ⟹ `Err(UnsupportedFn)` mapping to "List".
- `evalnum(String("hi"))` ⟹ `Err(UnsupportedFn)` mapping to "String".

### 6.2 Property-based tests (`proptest`)

- **Eval ∘ substitute equivalence (cross-doc):** for random `e` with all symbols bound to numeric values, `evalnum(substitute_many(e, bindings), {}) == evalnum(e, bindings)` (within tolerance for f64 ops). Jointly owned with `designs/substitution.md` §6.2.
- **Consistent with simplify** (the load-bearing test, also referenced in `designs/simplifier.md` §6.2): for random `e` with random rational bindings, `evalnum(simplify(e), bindings) ≈ evalnum(e, bindings)` to a tight tolerance (4 × ULP × max(|expected|, 1.0)).
- **Determinism:** the same `(e, bindings)` produces the same `f64` bit-for-bit on a single target, across 1000 repeated runs.
- **Short-circuits on first error:** for an input with a known `DivisionByZero` early in the walk, no `EvalError::UnboundSymbol` is raised even if later subterms contain unbound symbols.
- **DAG memoisation correctness:** for a DAG with deliberate sharing, the same f64 value is returned regardless of how many times a shared subterm is referenced, with the visited HashMap entry count equal to the distinct-node count.

### 6.3 Benchmarks (`criterion`)

| Benchmark | Target |
|-----------|--------|
| `evaluate_numeric(small_expr, full_bindings)` (≤ 10 nodes) | <50 µs |
| `evaluate_numeric(big_expr, full_bindings)` (≥ 1000 nodes) | <2 ms |
| `evaluate_numeric(numeric_only_expr)` (no bindings) | <100 µs |
| `subs + evaluate_numeric` pipeline (small expr; cross-doc with `designs/substitution.md` §6.3) | <200 µs combined |

The "big expr" benchmark is the regression target for the visited-HashMap memoisation: without it, repeated evaluation of a shared subterm would scale with sharing-multiplicity rather than distinct-node count.

### 6.4 Fuzz testing (`cargo-fuzz`)

- **Target:** `evaluate_numeric(parse(arbitrary_bytes), random_bindings)`. Asserts (a) no panics, (b) on success the result is a finite f64 (no Inf/NaN leakage — any Inf/NaN should have raised an EvalError), (c) on failure the error variant is one of the documented `EvalError` cases (no `Internal` errors should leak).
- **Seed corpus:** the legacy `.tst` files exercising numeric evaluation in `legacy/reduce-algebra-code-r7357-trunk/packages/numeric/` plus pathological inputs (very high-precision rationals near f64 overflow; deeply nested Add/Mul of bound symbols).
- **Run duration:** ≥1 hour per release (combined with the parser, simplifier, polynomial, substitute, and solver fuzz targets).

### 6.5 Golden-corpus tests (`pytest`)

A subset of `legacy/reduce-algebra-code-r7357-trunk/packages/`-level tests exercising numeric evaluation. For each `.tst` input, parse, run `evaluate_numeric`, render result, and compare against `.rlg`.

**Known intentional divergences from REDUCE** (recorded in the manifest with `# reason: ...` annotations):

- **Numeric NaN not propagated.** REDUCE's `numr!*` produces NaN for `0/0` and passes it through. Phase 1 raises `EvalError`. Documented per case.
- **`0^0 = 1`.** Matches both REDUCE and IEEE-754 — included for completeness.
- **Function tags translate to user-friendly names in errors.** Internal sentinel tags (`SENTINEL_EQ_NODE`, etc.) are not visible in the user-facing error string; REDUCE has no analog because it uses string-based dispatch throughout.

The curated set lives in `tests/golden/evalnum/` with the manifest mapping input file to expected output and the `# reason: ...` annotation per case.

---

## 7. Action Items

### Phase 1 — Core implementation

1. [ ] Create `crates/monomix-kernel/src/evalnum/mod.rs` exposing the public API (§2.1); wire `EvalError` into `KernelError` with `UnboundSymbol`, `DivisionByZero`, `LogOfNonPositive`, `SqrtOfNegative`, `DomainError`, `IntegerOverflow`, `UnsupportedFn` variants
2. [ ] Implement `evalnum/walk.rs` — fold-based evaluator, ExprNode dispatch with visited HashMap, sentinel translation (§3.1)
3. [ ] Implement `evalnum/coerce.rs` — `bigint_to_f64`, `rational_to_f64` with overflow checks (§3.2)
4. [ ] Implement `evalnum/funcs.rs` — built-in function dispatch table for sin/cos/tan/exp/log/sqrt/abs/asin/acos/atan with domain checks (§3.3.3)
5. [ ] Implement `pow` four-case logic (negative base + non-integer; zero base negative exp; standard; NaN result) in `evalnum/walk.rs` (§3.3)
6. [ ] Implement the `Bindings<'a>` borrow newtype with `from_session`, `from_dict`, `empty` constructors (§3.4)
7. [ ] Wire `evaluate_numeric` into the Python `Session` via PyO3 with the `subtree_size > 500` GIL-release threshold and a read-lock on the pool (§3.6)
8. [ ] Coordinate with `designs/substitution.md` §3.6 on the Session-resolve composition path so cycle errors produce clear messages regardless of which entry point the user takes (§3.5)
9. [ ] Add `pool.subtree_size(id)` if the DAG design hasn't yet (§3.6 depends on it; already an action item in `designs/simplifier.md` §3.8 — confirm shared)

### Phase 1 — Verification

10. [ ] Unit-test all transformations enumerated in §6.1, including the structured-error paths and the non-numeric-node sentinel translation
11. [ ] `proptest` evalnum-substitute equivalence (jointly owned with `designs/substitution.md` §6.2) + evalnum-simplify consistency + determinism + short-circuit + DAG memoisation correctness (§6.2)
12. [ ] `criterion` benchmarks including the big-expr regression guard (§6.3)
13. [ ] `cargo-fuzz` target for evaluate_numeric (§6.4)
14. [ ] Curate the golden-corpus `.tst`/`.rlg` subset for numeric evaluation, with a divergence manifest covering the intentional divergences in §6.5
15. [ ] Confirm SCOPE.md §1.8 invariants hold: evaluate_numeric returns Python float, EvalError on unbound symbols, no NaN leakage

### Phase 2 — Generalization (deferred)

16. [ ] Swap the rational coercion to `BigRational::to_f64` (§5.1)
17. [ ] Add a numerically-stable rewrite pass to the simplifier (`log(1+x)` → `ln_1p` etc.) — passive benefit to evalnum (§5.2)
18. [ ] Add MPFR backend behind `arb` feature flag with a `precision` parameter on `evaluate_numeric` (§5.3)
19. [ ] Implement `simplify_with_bindings` in the simplifier as the partial-numeric-evaluation API (§5.4)
20. [ ] Define the plugin contract for `evalnum_handler` callbacks on `FnTag::Custom` (§5.5); coordinate with the Phase 1.10 plugin system design
