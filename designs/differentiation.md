# Differentiation — System Design

**Component:** `monomix-kernel::diff`
**Status:** Design phase
**Date:** 2026-04-28
**References:** SCOPE.md §1.4, §1.7, §1.8, §0.5, §0.7; ADR-0001; ADR-0002;
`designs/expression-dag.md`; `designs/simplifier.md`; `designs/parser.md`

---

## 1. Requirements

### 1.1 Functional requirements

The differentiator computes the symbolic derivative of an expression with respect to a
symbol. It is invoked by the user via `df(f, x)` (parsed by the front-end per
SCOPE.md §1.2) and by other kernel components that need a derivative as a building block
(none in Phase 1; the §2 integrator and the §3.4 limits engine in later phases).

It must support every transformation listed in SCOPE.md §1.4:

- **Constants and free symbols.** `df(c, x) → 0` for every atom whose value does not depend
  on `x`. Numeric atoms (`SmallInt`, `BigInt`, `Rational`, `Float`) and `String` are always
  constant. Symbols other than `x` are constant in Phase 1 — there is no `depend(y, x)`
  declaration (deferred to Phase 2 §2.7).
- **The differentiation variable itself.** `df(x, x) → 1`.
- **Sums and differences (linearity).** `df(a + b, x) → df(a, x) + df(b, x)`.
  Naturally extends to n-ary `Add` because of the DAG's flattening invariant
  (`designs/expression-dag.md` §3.1 Invariant 4).
- **Products (Leibniz rule).** `df(a*b, x) → df(a, x)*b + a*df(b, x)`. For n-ary `Mul`
  with children `[c₀, c₁, …, cₙ₋₁]`, emit the sum of `n` terms, each with `df(cᵢ, x)`
  in the i-th slot and the original `cⱼ` (j ≠ i) in the rest.
- **Quotients.** `df(u/v, x) → (df(u, x)*v − u*df(v, x)) / v²`.
- **Power rule, integer/rational exponent constant in `x`.**
  `df(u^n, x) → n * u^(n−1) * df(u, x)`.
- **Power rule, base constant in `x`.** `df(a^v, x) → a^v * log(a) * df(v, x)` when `a`
  is constant in `x` and recognized as positive (`a` an integer ≥ 1, rational with positive
  numerator and denominator, or the symbol `e`). Other bases fall through to the general
  rule.
- **General power rule (logarithmic differentiation).**
  `df(u^v, x) → u^v * (v' * log(u) + v * u'/u)` where `u' = df(u, x)` and `v' = df(v, x)`.
  Triggered when both base and exponent depend on `x`.
- **Negation.** `df(-u, x) → -df(u, x)`.
- **Equality.** `df(lhs = rhs, x)` is rejected with `KernelError::DifferentiateEquation`
  — equations are not differentiable as a unit; the user should differentiate each side
  explicitly. Matches the original REDUCE behaviour
  ([diff.red:133-144](../legacy/reduce-algebra-code-r7357-trunk/packages/poly/diff.red))
  where `simpdf` calls `typerr` on a non-kernel/non-integer second argument.
- **Built-in functions (chain rule).** For every recognized `FnTag` (SCOPE.md §1.4: `sin`,
  `cos`, `tan`, `exp`, `log`, `sqrt`, `asin`, `acos`, `atan`), apply the standard
  derivative formula in the table of §3.4 and multiply by the chain-rule inner derivative.
- **Unknown / user-defined functions (`Fn(Custom(name), args)`).** Phase 1 has no notion
  of an unknown function depending on a specific variable. The differentiator emits a
  symbolic placeholder `df(Fn(Custom(name), args), x)` represented as
  `Fn(Custom("df"), [original, x])` — the same representation the parser produces for
  `df(...)` expressions that are not yet evaluable. This mirrors REDUCE's
  `mksq('df . u, 1)` fallback path
  ([diff.red:127](../legacy/reduce-algebra-code-r7357-trunk/packages/poly/diff.red))
  and keeps the AST closed under `differentiate`.
- **Iterated / partial derivatives via repeated application.** SCOPE.md §1.4 prescribes
  partial derivatives by repeated calls. The parser recognises `df(f, x, y)` and
  `df(f, x, n)` (n a positive integer) and lowers them to nested `Fn(Custom("df"), …)`
  applications in Phase 1. The differentiator is therefore a function of *one* variable
  per call; the parser's lowering composes them. Discussed in §3.6.
- **Idempotent on a constant input.** `df(c, x)` for any expression `c` that does not
  contain `x` returns `pool.zero` — a single `ExprId` regardless of `c`'s structure.

### 1.2 Non-functional requirements

| Requirement | Target | Rationale |
|-------------|--------|-----------|
| Latency on a 20-term univariate polynomial | <50 ms wall-clock from Python | SCOPE.md Phase 1 success criterion |
| Latency on a single elementary-function expression (≤30 nodes) | <1 ms | REPL feel |
| Termination on any input | Always — recursion bounded by DAG depth | Correctness |
| Determinism | Same `(expr, var)` ⇒ same output `ExprId` across runs | Tests + cache reproducibility |
| `Send + Sync` | Required | Kernel rule (ADR-0002), MCP (§0.5) |
| GIL release | Yes, for inputs >500 nodes | SCOPE.md §0.5 — release if expected >1 ms |
| No `unsafe` | Required | Kernel rule (ADR-0002) |
| Plugin extensibility | Custom `FnTag::Custom` derivatives registered via the §1.10 plugin hook | SCOPE.md §1.10 |

### 1.3 Constraints

- The differentiator reads expressions via `&ExprPool` and creates new ones via
  `&mut ExprPool::*` constructors. **It never constructs `ExprNode` directly.** This
  preserves the eager-normalization invariants of `designs/expression-dag.md` §3.1 — in
  particular `0 * u → 0`, `1 * u → u`, `u + 0 → u` are guaranteed at intern time, so
  formulae below that read as if generating a lot of trivial sub-products actually emit
  compact terms.
- The differentiator is **stateless across calls**: all working state is on the stack
  or in caller-owned scratch buffers (the per-call memoization cache).
- The differentiator is **bounded in Phase 1**: only the rules enumerated in §1.1 are
  applied. No automatic simplification of the result (SCOPE.md §1.4); no integration
  short-cuts (Phase 2 §2.1); no series-expansion-based linearisation (Phase 2 §2.4).
- **No automatic simplification post-differentiation.** Per SCOPE.md §1.4 and the
  simplifier design (`designs/simplifier.md` §1.3), `simplify` is invoked only when the
  caller asks for it. The differentiator deliberately produces structurally faithful but
  often visually noisy output (e.g., `1*x + cos(x)*0`) — it relies on the pool's eager
  normalizations to collapse the trivially-zero/one cases at intern time, but does not
  run the full simplifier.

### 1.4 What the differentiator is **not**

To pin scope precisely:

- It is **not the parser.** The parser handles `df(f, x, y, n)` syntax and lowers it.
- It is **not the simplifier.** Output may contain unsimplified products and sums.
- It is **not the substitution engine.** It cannot substitute `df(u, x)` with a known
  value of `du/dx` — that's `Session`-level work.
- It does **not** track differentiation under the integral sign, commute mixed partials
  intelligently, or know about `depend(y, x)` declarations. All Phase 2+ (SCOPE.md §2.7).

---

## 2. High-Level Design

### 2.1 Public API

```rust
/// Differentiate `expr` with respect to symbol `var`. Returns a new ExprId in the same
/// pool. The result is *not* simplified — call `simplify::simplify` afterwards if a
/// canonical form is desired (SCOPE.md §1.4).
///
/// `cache` is a caller-owned per-call memoization scratch buffer keyed on the input
/// `ExprId`. It is *not* a `Session`-wide cache: the result depends on the `var`
/// argument, and stale entries from a previous `df(.., y)` would be wrong for `df(.., x)`.
/// See §3.2 for the cache shape.
///
/// Errors: `KernelError::DifferentiateEquation` if `expr` is an `Eq(_, _)`;
/// `KernelError::NotASymbol` if `var` is not a `Symbol`.
pub fn differentiate(
    pool: &mut ExprPool,
    cache: &mut DiffCache,
    expr: ExprId,
    var: ExprId,
) -> Result<ExprId, KernelError>;

/// Convenience wrapper for one-shot callers — allocates a fresh cache per call.
pub fn differentiate_fresh(
    pool: &mut ExprPool,
    expr: ExprId,
    var: ExprId,
) -> Result<ExprId, KernelError>;

/// Per-call memoization scratch. Keyed on input `ExprId` only (not (id, var)) because
/// each `differentiate` call fixes a single `var` and the cache lives only for that call.
/// Cleared by the caller between calls if reused across a multi-step pipeline.
pub struct DiffCache { /* FxHashMap<ExprId, ExprId> */ }

impl DiffCache {
    pub fn new() -> Self;
    pub fn clear(&mut self);
}
```

The `var: ExprId` parameter is required to be a `Symbol`. Differentiating with respect
to a compound expression (`df(f, x+1)`) is a category error rejected at the boundary —
this matches REDUCE's `simpdf` ([diff.red:131-145](../legacy/reduce-algebra-code-r7357-trunk/packages/poly/diff.red))
which only accepts a kernel as the second argument. Phase 2+ may relax this for
`depend`-style indirect dependencies.

### 2.2 Component diagram

```
                     ExprId (root) + ExprId (var)
                                │
                                ▼
                       ┌─────────────────┐
                       │   driver.rs     │   memoized recursive descent
                       │  (DiffCache)    │
                       └────────┬────────┘
                                │ for each node, dispatch by ExprNode variant:
                                ▼
   ┌────────────────────────────────────────────────────────────┐
   │                                                            │
   ▼              ▼              ▼              ▼              ▼
┌───────┐    ┌────────┐    ┌────────┐    ┌────────┐    ┌──────────┐
│ atoms │    │ Add    │    │ Mul    │    │ Pow    │    │ Fn(tag,  │
│ Sym/  │    │ (line- │    │ (Leib- │    │ (4-way │    │  args)   │
│ Const │    │  arity)│    │  niz)  │    │  rule) │    │ chain    │
└───┬───┘    └────┬───┘    └────┬───┘    └────┬───┘    └────┬─────┘
    │             │              │             │             │
    │             │              │             │             ▼
    │             │              │             │     ┌─────────────────┐
    │             │              │             │     │ derivative      │
    │             │              │             │     │ table           │
    │             │              │             │     │  (FnTag → fn)   │
    │             │              │             │     │  + plugin hook  │
    │             │              │             │     └─────────────────┘
    │             │              │             │
    └─────────────┴──────────────┴─────────────┘
                                │
                                ▼
                    pool.add / pool.mul / pool.pow / pool.div / ...
                    (re-interned through ExprPool's normalizing constructors —
                     0/1/x identities collapse here, not in this module)
                                │
                                ▼
                    new ExprId returned to driver, memoized in cache
```

### 2.3 Module layout

```
crates/monomix-kernel/src/diff/
├── mod.rs           — public API (`differentiate`, `differentiate_fresh`),
│                      KernelError mapping
├── driver.rs        — recursive descent, memoization cache, dispatch
├── arith.rs         — Add/Mul/Pow/Div/Neg rules
├── functions.rs     — chain-rule plumbing for Fn nodes
├── table.rs         — built-in derivative table (FnTag → derivative builder)
├── plugin.rs        — registration entry point for plugin-supplied derivatives
└── tests.rs
```

The split mirrors the convention used by `simplify/` (`designs/simplifier.md` §2.3) and
`parser/`: a focused dispatch driver, per-concern rule modules, rules-as-data separated
from the engine that applies them. The Phase 2 generalization (introduce `depend`
declarations and support general `df(unknown, x)` evaluation) is additive in `table.rs`
and `plugin.rs` and does not touch `driver.rs`.

### 2.4 Single-pass — no fixed-point loop

Unlike the simplifier (`designs/simplifier.md` §2.4) which runs a bounded fixed-point
loop, the differentiator is a **single bottom-up pass**. There is no rule interaction
that could expose new opportunities at the root after a pass — `df` is a homomorphism
on the AST shape (each node maps to a fixed pattern of derivatives of its children).
The structural recursion completes in exactly one walk; the only "iteration" comes from
shared subexpressions in the DAG, which are visited once via the memoization cache.

A second pass *could* reduce the structural noise (the `1*x + cos(x)*0` artifacts), but
that's the simplifier's job — composing `simplify ∘ differentiate` is the user-facing
pattern, and intentionally separating the two keeps the differentiator's output
predictable for downstream consumers (e.g., the Phase 2 integrator that performs its own
pattern matching).

### 2.5 Data flow through a single call

```
  input (root, var)
        │
        ▼
  cache.get(root)?  ─── hit ──► return cached ExprId
        │ miss
        ▼
  read &ExprNode from pool
        │
        ▼
  variant-specific dispatch:
   ┌─ atom (Symbol|Int|Rational|Float|String)
   │       → if Symbol == var, return pool.one
   │       → else return pool.zero
   │
   ├─ Add(children)  → sum of recursive diff over each child
   │                   (linearity; n-ary children handled in one shot)
   │
   ├─ Mul(children)  → Σᵢ ∏ⱼ (j == i ? df(cⱼ, var) : cⱼ)   [Leibniz n-ary]
   │
   ├─ Pow(b, e)      → 4-way dispatch by which side depends on var
   │                   (see §3.3)
   │
   ├─ Div(n, d)      → quotient rule
   │
   ├─ Neg(u)         → -df(u, var)
   │
   ├─ Eq(_, _)       → KernelError::DifferentiateEquation
   │
   ├─ Fn(tag, args)  → chain rule using table.rs / plugin.rs
   │                   (see §3.4)
   │
   └─ List(items)    → list of derivatives (componentwise)
        │
        ▼
  cache.insert(root → result)
        │
        ▼
  return result ExprId
```

The eager normalizations baked into `ExprPool` (`designs/expression-dag.md` §3.1
Invariant 5) mean the formulae above produce minimal output for trivial inputs without
the differentiator emitting any special cases:

- `df(0, x)` reads as `Add([df(0, x)])` only conceptually; the atom branch returns
  `pool.zero` directly.
- A Leibniz term whose `df(cᵢ, var)` is `pool.zero` is multiplied through `pool.mul(...)`
  which collapses the entire factor to `pool.zero`; that term is then dropped by
  `pool.add(...)`.
- A chain-rule outer · inner where `inner = df(args[0], var) = pool.zero` yields
  `pool.zero` after the multiplication.

The differentiator therefore reads as the textbook formulae and the pool handles the
algebraic minimization at no extra source-code cost.

---

## 3. Deep Dive

### 3.1 Driver (`driver.rs`)

```rust
pub fn differentiate(
    pool: &mut ExprPool,
    cache: &mut DiffCache,
    expr: ExprId,
    var: ExprId,
) -> Result<ExprId, KernelError> {
    if !matches!(pool.get(var), ExprNode::Symbol(_)) {
        return Err(KernelError::NotASymbol);
    }
    diff_node(pool, cache, expr, var)
}

fn diff_node(
    pool: &mut ExprPool,
    cache: &mut DiffCache,
    expr: ExprId,
    var: ExprId,
) -> Result<ExprId, KernelError> {
    if let Some(&cached) = cache.map.get(&expr) {
        return Ok(cached);
    }

    let result = match pool.get(expr) {
        // --- Atoms ---
        ExprNode::Symbol(_) =>
            if expr == var { pool.one } else { pool.zero },
        ExprNode::SmallInt(_) | ExprNode::BigInt(_)
        | ExprNode::Rational(_) | ExprNode::Float(_)
        | ExprNode::String(_) => pool.zero,

        // --- Linearity ---
        ExprNode::Add(children) => {
            let kids = children.clone();   // small Box<[ExprId]>; cheap clone of a (ptr,len)
            let mut diffed = Vec::with_capacity(kids.len());
            for c in kids.iter() {
                diffed.push(diff_node(pool, cache, *c, var)?);
            }
            pool.add(diffed)               // pool drops zero-children automatically
        }
        ExprNode::Neg(u) => {
            let u = *u;
            let du = diff_node(pool, cache, u, var)?;
            pool.neg(du)                   // pool collapses -0 → 0, --u → u
        }

        // --- Product, quotient, power: see §3.3 ---
        ExprNode::Mul(children) => arith::diff_mul(pool, cache, children.clone(), var)?,
        ExprNode::Div(n, d)    => arith::diff_div(pool, cache, *n, *d, var)?,
        ExprNode::Pow(b, e)    => arith::diff_pow(pool, cache, *b, *e, var)?,

        // --- Equation: rejected ---
        ExprNode::Eq(_, _) =>
            return Err(KernelError::DifferentiateEquation),

        // --- Function application: chain rule via table ---
        ExprNode::Fn(tag, args) =>
            functions::diff_fn(pool, cache, *tag, args.clone(), var)?,

        // --- List: componentwise ---
        ExprNode::List(items) => {
            let items = items.clone();
            let mut diffed = Vec::with_capacity(items.len());
            for it in items.iter() {
                diffed.push(diff_node(pool, cache, *it, var)?);
            }
            pool.list(diffed)
        }
    };

    cache.map.insert(expr, result);
    Ok(result)
}
```

**Why a custom driver, not `map_bottom_up` from the DAG design.** The expression-DAG
`map_bottom_up` (`designs/expression-dag.md` §3.6) applies a per-node closure that takes
only the node — no extra context. The differentiator needs `var` plus the full
recursive-descent structure (Leibniz on `Mul` interleaves *original* children with
*differentiated* children — a generic bottom-up rewrite can't express that without
recovering the original children from the pool). A bespoke driver is clearer and shorter
than fitting the rule into `map_bottom_up`'s contract. Memoization is implemented
directly in the driver using the same `FxHashMap<ExprId, ExprId>` shape so future
unification with the simplifier's traversal infrastructure is straightforward.

**Stack depth.** The driver is recursive on the DAG. Maximum recursion depth equals
expression depth, which the parser bounds at 256 (`designs/parser.md` §2.1 grammar +
practical input). For pathological inputs constructed programmatically (deep `Add` chains
formed at runtime, although the pool's flattening invariant collapses these), a Phase 2
revision can convert `diff_node` to an explicit stack — the cache-map shape is unchanged.

### 3.2 Memoization cache

The cache is a per-call `FxHashMap<ExprId, ExprId>`. Why per-call rather than
session-scoped:

- **The result is var-dependent.** `df(x*y, x) = y` and `df(x*y, y) = x` share the input
  `ExprId` for `x*y` but produce different outputs. A session-wide cache would need a
  composite key `(ExprId, var: ExprId)`; the extra hash work is wasted because in a
  typical pipeline (`simplify(df(simplify(e), x))`) the input expressions to `df` are
  freshly produced and won't be hit on the next call.
- **It mirrors the simplifier's caller-owned `cache` parameter** (`designs/simplifier.md`
  §3.1) so multi-step pipelines can either share or isolate scratch space deliberately.
  A future `Session::diff_cache` is additive.

The cache pays off whenever a subexpression is shared in the DAG. For the canonical
worst case — `df((x+1)^10, x)` after `expand` produces a sum with hundreds of terms but
only ~40 unique sub-monomials — the cache reduces the effective work from "sum of all
node visits" to "number of distinct sub-monomials × constant" (see §6.3 benchmark).

**No eviction policy.** The cache lives only for the call duration; it is dropped along
with the stack frame. For very large inputs (>10⁶ nodes), the cache may grow into the
megabytes range — acceptable for a one-shot operation; comparable to the simplifier's
`SimplifyCache`. Long-running scenarios where one cache should cover many calls
(e.g. computing Jacobians) are handled by the caller passing the same `DiffCache`
instance and `clear()`ing between calls if it grows beyond a threshold.

### 3.3 Arithmetic rules (`arith.rs`)

#### Mul — n-ary Leibniz

```rust
pub fn diff_mul(
    pool: &mut ExprPool,
    cache: &mut DiffCache,
    children: Box<[ExprId]>,
    var: ExprId,
) -> Result<ExprId, KernelError> {
    // For Mul([c0, c1, ..., c_{n-1}]):
    //   df = Σ_i  c0 * c1 * ... * df(ci, var) * ... * c_{n-1}
    // Pre-compute df of each child once; build n products.
    let n = children.len();
    let mut dchildren = Vec::with_capacity(n);
    for c in children.iter() {
        dchildren.push(diff_node(pool, cache, *c, var)?);
    }

    // Fast path: every dchild is zero ⇒ df = 0. Saves the n products.
    if dchildren.iter().all(|&d| d == pool.zero) {
        return Ok(pool.zero);
    }

    let mut terms = Vec::with_capacity(n);
    for i in 0..n {
        if dchildren[i] == pool.zero { continue; }   // skip the all-zero term
        let mut factors = Vec::with_capacity(n);
        for j in 0..n {
            factors.push(if j == i { dchildren[i] } else { children[j] });
        }
        terms.push(pool.mul(factors));
    }
    Ok(pool.add(terms))
}
```

**Cost.** O(n²) factors emitted for an n-ary `Mul` — unavoidable for Leibniz on n
arguments. Typical inputs (n ≤ 4 from human-written algebra) are negligible. For n=20
(post-`expand` polynomial), ~400 multiplication slots are interned but the pool's
flattening + dedup means many are re-collapsed to the same `ExprId`. Empirical 50-term
benchmark in §6.3 confirms <50 ms.

**Why not factor out the binary form `df(a*b, x) = a'*b + a*b'` and recurse.**
`Mul([a, b, c])` is *one* node in the DAG; treating it as `Mul([a, Mul([b, c])])` would
require an extra rebuild (and the pool's flattening would cancel that re-nesting at intern
time anyway). The n-ary formulation matches the DAG shape and emits exactly the terms
the user expects to read.

#### Div — quotient rule

```rust
pub fn diff_div(
    pool: &mut ExprPool,
    cache: &mut DiffCache,
    num: ExprId,
    den: ExprId,
    var: ExprId,
) -> Result<ExprId, KernelError> {
    let dn = diff_node(pool, cache, num, var)?;
    let dd = diff_node(pool, cache, den, var)?;

    // df(u/v) = (u'*v - u*v') / v^2
    let two = pool.integer(2);
    let v_sq = pool.pow(den, two);
    let dn_v = pool.mul(vec![dn, den]);
    let u_dd = pool.mul(vec![num, dd]);
    let neg_u_dd = pool.neg(u_dd);
    let num_out = pool.add(vec![dn_v, neg_u_dd]);
    Ok(pool.div(num_out, v_sq))
}
```

The `pool.div(num_out, v_sq)` call inherits the pool's normalization: if `num_out`
collapses to zero (both `dn` and `dd` zero), the whole expression becomes
`pool.div(zero, v_sq)` which the pool reduces to `pool.zero` via its eager
identities. No special-case code is needed.

**Division by zero is not detected here.** That detection lives in the simplifier
(`designs/simplifier.md` §3.5) via `KernelError::DivisionByZero` when `den` is recognized
as zero. The differentiator's job is to emit the correct symbolic formula; if the user
constructs `df(1/0, x)`, the offending `1/0` came from a parser or prior call and
should have raised earlier.

#### Pow — four-way dispatch

```rust
pub fn diff_pow(
    pool: &mut ExprPool,
    cache: &mut DiffCache,
    base: ExprId,
    exp: ExprId,
    var: ExprId,
) -> Result<ExprId, KernelError> {
    let base_has_var = pool.contains_symbol(base, var);
    let exp_has_var  = pool.contains_symbol(exp, var);

    match (base_has_var, exp_has_var) {
        // 1. Neither side depends on var → derivative is zero.
        (false, false) => Ok(pool.zero),

        // 2. Only the base depends on var → power rule.
        //    df(u^n, x) = n * u^(n-1) * u'
        (true, false) => {
            let du = diff_node(pool, cache, base, var)?;
            let one = pool.one;
            let neg_one = pool.minus_one;
            let exp_minus_1 = pool.add(vec![exp, neg_one]);
            let u_pow = pool.pow(base, exp_minus_1);
            Ok(pool.mul(vec![exp, u_pow, du]))
        }

        // 3. Only the exponent depends on var → exponential rule.
        //    df(a^v, x) = a^v * log(a) * v'
        //    Sound for any a interpreted as positive; for negative or symbolic a we
        //    still emit this — it is the analytic continuation. REDUCE matches.
        (false, true) => {
            let dv = diff_node(pool, cache, exp, var)?;
            let log_a = pool.func(FnTag::Log, vec![base]);
            let a_pow_v = pool.pow(base, exp);
            Ok(pool.mul(vec![a_pow_v, log_a, dv]))
        }

        // 4. Both sides depend on var → general (logarithmic) power rule.
        //    df(u^v, x) = u^v * (v' * log(u) + v * u'/u)
        (true, true) => {
            let du = diff_node(pool, cache, base, var)?;
            let dv = diff_node(pool, cache, exp, var)?;
            let log_u = pool.func(FnTag::Log, vec![base]);
            let dv_log_u = pool.mul(vec![dv, log_u]);
            let du_over_u = pool.div(du, base);
            let v_du_over_u = pool.mul(vec![exp, du_over_u]);
            let inner = pool.add(vec![dv_log_u, v_du_over_u]);
            let u_pow_v = pool.pow(base, exp);
            Ok(pool.mul(vec![u_pow_v, inner]))
        }
    }
}
```

The `pool.contains_symbol` predicate is provided by the expression DAG
(`designs/expression-dag.md` §3.5) and runs in O(distinct-subexpressions) thanks to the
DAG's structural sharing — not O(tree-size).

**Why not always use the general (case 4) formula.** Case 2 (the textbook power rule) is
the overwhelmingly common case (polynomial differentiation), and emitting `log(u)` for
inputs like `df(x^3, x)` would be visually wrong and only collapse on `simplify`. The
four-way split keeps the output recognizable for typical inputs.

**Branch cuts in case 3.** `df(a^v) = a^v * log(a) * v'` is the analytic-continuation
formula. For real `a < 0`, `log(a)` is not real-valued — but Phase 1 has no complex
numbers (SCOPE.md §1.1). Three options were considered:

- Emit `log(a)` unconditionally and accept that the formula can't be `evaluate_numeric`d
  for negative bases without complex support. **Chosen.** Matches REDUCE's behaviour and
  keeps the symbolic differentiator decoupled from numeric evaluation.
- Restrict case 3 to bases that statically prove positive (`a` an integer ≥ 1, the symbol
  `e`, `pi`, etc.) and fall through to general case 4 otherwise. Rejected because case 4
  also produces `log(u)` — there is no escape from the `log` for variable exponents.
- Reject with `UnsupportedError` on negative-or-unknown-sign base. Rejected because it
  would prevent legal symbolic manipulations like `df((-1)^x, x)` from completing in
  symbolic form.

The user is responsible for evaluating the result in a domain where it is well-defined;
Phase 3 (complex numbers, SCOPE.md §3.1) closes the loop.

### 3.4 Built-in function table (`table.rs`)

```rust
/// One row of the derivative table: given the args of `f(args...)`, return the outer
/// derivative — i.e. the value of f'(args[0]) (NOT yet multiplied by the chain-rule
/// inner derivative). The driver multiplies by the inner derivative.
///
/// Every builder takes `&mut ExprPool` and the original args slice; it returns a single
/// `ExprId` representing the outer factor.
pub type DerivativeBuilder =
    fn(&mut ExprPool, &[ExprId]) -> Result<ExprId, KernelError>;

pub fn lookup(tag: FnTag) -> Option<DerivativeBuilder> {
    match tag {
        FnTag::Sin   => Some(deriv_sin),
        FnTag::Cos   => Some(deriv_cos),
        FnTag::Tan   => Some(deriv_tan),
        FnTag::Exp   => Some(deriv_exp),
        FnTag::Log   => Some(deriv_log),
        FnTag::Sqrt  => Some(deriv_sqrt),
        FnTag::Asin  => Some(deriv_asin),
        FnTag::Acos  => Some(deriv_acos),
        FnTag::Atan  => Some(deriv_atan),
        FnTag::Abs   => None,                  // §3.7 — Phase 2
        FnTag::Custom(_) => None,              // see §3.5
    }
}
```

Standard outer derivatives (one row per `FnTag`). `u` denotes `args[0]`; the Phase 1
table is unary-only (every standard function in SCOPE.md §1.4 is unary; `atan2` is Phase
2). Identities are taken directly from REDUCE's `elem.red`
([elem.red:840-880](../legacy/reduce-algebra-code-r7357-trunk/packages/alg/elem.red)),
which is the legacy oracle for the §6.5 golden corpus check:

| `FnTag`  | Outer derivative                |
|----------|---------------------------------|
| `Sin`    | `cos(u)`                        |
| `Cos`    | `-sin(u)`                       |
| `Tan`    | `1 + tan(u)^2`                  |
| `Exp`    | `exp(u)`                        |
| `Log`    | `1/u`                           |
| `Sqrt`   | `1 / (2 * sqrt(u))`             |
| `Asin`   | `1 / sqrt(1 - u^2)`             |
| `Acos`   | `-1 / sqrt(1 - u^2)`            |
| `Atan`   | `1 / (1 + u^2)`                 |

**Tan choice.** REDUCE uses `1 + tan(x)^2` rather than `sec(x)^2` because Phase 1 has no
`Sec` `FnTag` and the equivalent form keeps the AST closed under the existing tag set.
This is the "F.J. Wright preference for integration" alluded to in `elem.red:858`.

**Per-function builders.** Each row is a small function:

```rust
fn deriv_sin(pool: &mut ExprPool, args: &[ExprId]) -> Result<ExprId, KernelError> {
    debug_assert_eq!(args.len(), 1);
    Ok(pool.func(FnTag::Cos, vec![args[0]]))
}

fn deriv_log(pool: &mut ExprPool, args: &[ExprId]) -> Result<ExprId, KernelError> {
    debug_assert_eq!(args.len(), 1);
    Ok(pool.div(pool.one, args[0]))
}

// ... etc.
```

The chain rule is applied at the call site:

```rust
// functions.rs
pub fn diff_fn(
    pool: &mut ExprPool,
    cache: &mut DiffCache,
    tag: FnTag,
    args: Box<[ExprId]>,
    var: ExprId,
) -> Result<ExprId, KernelError> {
    // Phase 1: every entry in the table is unary.
    debug_assert_eq!(args.len(), 1, "Phase 1 derivative table is unary-only");

    // Inner derivative: df(u, var). If zero, the chain rule short-circuits.
    let du = diff_node(pool, cache, args[0], var)?;
    if du == pool.zero {
        return Ok(pool.zero);
    }

    // Look up outer derivative.
    let builder = match table::lookup(tag).or_else(|| plugin::lookup(tag)) {
        Some(b) => b,
        None => return symbolic_df_placeholder(pool, tag, args, var),
    };
    let outer = builder(pool, &args)?;
    Ok(pool.mul(vec![outer, du]))
}
```

The `du == pool.zero` short-circuit is more than an optimisation — it's the *clean*
chain-rule output when the inner expression doesn't depend on `var`. Without it the
output would be `pool.mul([cos(u), 0])` which the pool collapses to `0` anyway, but
short-circuiting saves the table lookup and the multiplication.

### 3.5 Custom / user functions

For `Fn(Custom(name), args)` where the function isn't in the built-in table and no
plugin has registered a derivative, the differentiator emits a symbolic placeholder:

```rust
fn symbolic_df_placeholder(
    pool: &mut ExprPool,
    tag: FnTag,
    args: Box<[ExprId]>,
    var: ExprId,
) -> Result<ExprId, KernelError> {
    // Reconstruct the original function call and wrap it in df(_, var).
    // `pool.func_named(name, args)` is a thin convenience over `intern_str` + `pool.func`
    // that this module needs and which the expression-dag module exposes alongside
    // `pool.symbol`/`pool.string` (both already round-trip the IndexSet — adding a
    // `func_named` accessor keeps the InternedStr type private to the kernel).
    let original = pool.func(tag, args.to_vec());
    Ok(pool.func_named("df", vec![original, var]))
}
```

This matches REDUCE's `mksq('df . u, 1)` ([diff.red:127](../legacy/reduce-algebra-code-r7357-trunk/packages/poly/diff.red))
fallback and keeps the AST closed: `differentiate(differentiate(f(g(x)), x), x)` always
returns *some* `ExprId`, even when no rule fires. The user-visible representation is the
parser-equivalent form `df(f(g(x)), x)`, so the result is round-trippable through the
parser — important for the §6.5 golden corpus.

**Pool API note.** The `pool.func_named(name: &str, args)` accessor referenced here is a
small addition the expression-dag module needs to expose — it interns the name (the same
work `pool.symbol` already does internally) and produces a `Fn(FnTag::Custom(...), ...)`
node. Listed as an action item in §7.

**Plugin extension.** The §1.10 plugin contract (SCOPE.md §1.10) allows a plugin to
register a `DerivativeBuilder` for a `Custom(name)` tag:

```rust
// plugin.rs
pub fn register(
    name: InternedStr,
    builder: DerivativeBuilder,
) -> Result<(), PluginError>;

pub fn lookup(tag: FnTag) -> Option<DerivativeBuilder>;
```

Registration is process-global because `FnTag::Custom(InternedStr)` is content-addressed
on the string-interning table, which is itself per-`Session`. The plugin host is
responsible for re-binding the string per session. This is a Phase 1 wrinkle — the
Phase 2 plugin design (after observing real plugin code) may move the registration to
`Session` scope. The internal "stdlib" plugin (SCOPE.md §1.10) does not exercise this
hook in Phase 1; the standard functions live in the built-in table for performance, not
because they have to.

### 3.6 Iterated and partial derivatives

The parser lowers `df(f, x, y)` (mixed partial) and `df(f, x, n)` for integer `n ≥ 1`
(repeated) into nested `Fn(Custom("df"), …)` applications:

```
df(f, x, y)        →   df(df(f, x), y)
df(f, x, 2)        →   df(df(f, x), x)
df(f, x, y, x)     →   df(df(df(f, x), y), x)
```

Each lowering step wraps the result in another `Fn(Custom("df"), [inner, var])` call.
The Phase 1 differentiator therefore sees only single-variable `df` invocations. This
matches REDUCE's `simpdf` outer loop ([diff.red:122-156](../legacy/reduce-algebra-code-r7357-trunk/packages/poly/diff.red))
which iterates `diffsq(u, x)` once per kernel argument.

**Caveat — mixed partials are not auto-commuted.** `df(df(f, x), y)` and `df(df(f, y), x)`
produce structurally different `ExprId`s in Phase 1 even when Clairaut's theorem
guarantees they're equal. The simplifier does not commute them either. REDUCE has a
`commutedf` switch for this; Phase 2 (SCOPE.md §2.6 advanced simplification) will add the
analogous rule. Until then, callers expecting equality of mixed partials must canonicalize
the variable order before constructing the call.

**Why not handle iteration in the differentiator.** Treating `df` as a special-cased AST
shape in the differentiator would couple it to the parser's lowering and special-case
the `Fn(Custom("df"), …)` form. Keeping the differentiator a one-variable function and
delegating multiplicity to the parser/AST keeps each component's contract narrow and
matches the REDUCE separation between `simpdf` (iteration) and `diffsq` (one step).

### 3.7 Functions deliberately *not* differentiated in Phase 1

| Function | Rationale |
|----------|-----------|
| `abs(x)`  | Derivative is `sign(x)`; `sign` is not in the Phase 1 atom set. The differentiator emits the symbolic-placeholder form per §3.5. |
| `int(...)`| Phase 2 stub; SCOPE.md §1.4. The differentiator emits a placeholder so the outer `df` round-trips through the parser. Phase 2 will add `df(int(f, x), x) → f` and the more general "differentiation under the integral sign" handling that REDUCE's `dfform_int` provides ([diff.red:442](../legacy/reduce-algebra-code-r7357-trunk/packages/poly/diff.red)). |
| `factor(...)` | Phase 2 stub; treated as opaque, emits placeholder. |
| `solve(...)`, `simplify(...)`, `expand(...)` | These are kernel operations, not algebraic functions of an argument; the parser routes them to dedicated dispatch and they should not appear inside a `df` argument. The differentiator does not special-case them — they would hit the placeholder path. |

The placeholder path means `df` is *total* on the AST: every well-formed input produces
an `ExprId` (or a deliberate `KernelError` for `Eq`). No path panics, no path enters an
infinite loop.

### 3.8 Error handling

| Error | Source | When |
|-------|--------|------|
| `KernelError::NotASymbol` | `var` argument | `var` is not a `Symbol` (e.g. `df(f, x+1)`) |
| `KernelError::DifferentiateEquation` | `expr` is `Eq(_, _)` | The user wrote `df(a = b, x)` |
| `KernelError::DivisionByZero` | propagated from `pool.div` only if a future pool change adds runtime checks | Currently the pool does not check; this row is forward-compatible |

There is no `KernelError::UnsupportedDerivative`. Unknown functions become symbolic
placeholders (§3.5) — that is *not* an error condition in Phase 1. Callers wanting strict
behaviour can post-scan the result for `Fn(Custom("df"), …)` nodes.

---

## 4. Trade-off Analysis

### 4.1 Symbolic differentiation vs. forward-mode AD vs. dual numbers

**Chosen: symbolic differentiation that produces a new AST.**

| Dimension | Symbolic AST | Forward-mode AD | Dual numbers |
|-----------|--------------|------------------|--------------|
| Output usable for further symbolic manipulation | yes | no — value-only | no — value-only |
| Cost on `df(f, x)` for a single `x` | O(distinct-subexprs) | O(eval cost of f) | O(eval cost of f) |
| Cost on Jacobian (m vars) | m × symbolic | m × forward | m × forward |
| Required to implement Phase 2 integrator | yes | no | no |
| Required to implement REPL `df(f, x)` semantics | yes | no | no |
| Floats in output unless input had them | no | yes (numeric only) | yes (numeric only) |

The user-facing operation is **algebraic** differentiation per SCOPE.md §1.4 — the result
is an expression to inspect, simplify, substitute, plot, integrate. Forward-mode AD and
dual numbers are *numeric* techniques that produce a value at a point, not a symbolic
derivative. They are out of scope for the same reason `evaluate_numeric` (SCOPE.md §1.8)
is a separate path — symbolic and numeric live in different modes and don't silently mix.

A future numeric differentiation path (e.g., for `evaluate_numeric(df(f, x), {x: 3.0})`)
would be implemented by *first* producing the symbolic derivative via this module, *then*
substituting and numerically evaluating. Forward AD is a Phase 3+ candidate if the
overhead of the symbolic-then-numeric pipeline ever becomes a bottleneck.

### 4.2 Fold trivial 0/1 in the differentiator vs. defer to the pool

**Chosen: defer to the pool.**

Every formula in §3.3 looks naive — e.g. the Leibniz code emits an n-term sum without
checking which `dchildren[i]` are zero. The pool's `pool.add(...)` and `pool.mul(...)`
constructors collapse zero summands and zero factors at intern time
(`designs/expression-dag.md` §3.1 Invariant 5), so the visible output already eliminates
the trivial cases.

Trade-off: a `pool.mul([a, 0, b])` allocation and intern still happens before the pool
returns `pool.zero` — there is *some* wasted work. Counter-argument: the alternative is
duplicating the trivial-case logic in the differentiator and risking the two paths
drifting (the pool's normalizations and the differentiator's pre-checks could disagree
under maintenance). The §6.3 benchmark confirms the wasted work is below the noise floor.

The one place we *do* short-circuit is `du == pool.zero` in `diff_fn` (§3.4), because
that saves a chain-of-rule lookups (table dispatch, custom search, builder call) — a
deeper saving than just one intern.

### 4.3 Per-call cache vs. session-scoped cache

**Chosen: per-call cache (caller-owned `DiffCache`).**

A session-scoped cache keyed on `(ExprId, ExprId)` (the var) is conceivable but rejected:

- The hit rate across calls is low. Pipelines like `simplify(df(simplify(e), x))`
  re-intern intermediate `ExprId`s, so the input to the second `df` is rarely the same
  `ExprId` as a previous call.
- It would compete with the simplifier's `SimplifyCache` for memory budget without an
  obvious shared eviction strategy.
- Per-call cache is one hashmap per call — the allocation cost is negligible against the
  diff work itself, and the GC story is trivial (the cache is freed when the call
  returns).

The caller-owned cache parameter (over `differentiate_fresh`) is the same shape used by
the simplifier — it lets multi-step pipelines reuse a single allocation across calls
when they want to.

### 4.4 Treating `df` as a closed AST node vs. always reducing

**Chosen: leave `Fn(Custom("df"), [f, var])` as a closed AST node when no rule applies.**

The alternative — propagate a typed `KernelError::UnsupportedDerivative` whenever a
user-defined function is encountered — was rejected because it makes `differentiate`
*partial* on the AST. Many legitimate users (Phase 1.5 MCP server, plugin-authored rules)
construct expressions containing custom functions and want a derivative AST back even if
some terms remain symbolic. The closed-AST choice matches REDUCE's behaviour and the
parser's representation: `df(f(x), x)` remains parseable and printable through every
phase boundary.

---

## 5. Scale, Limits, and Future Work

### 5.1 Performance characteristics

For an input DAG of `N` total nodes with `K` distinct subexpressions (`K ≤ N`, often
`K ≪ N` after `expand`), one `differentiate` call performs `O(K)` recursive node visits
plus `O(K)` pool insertions for the resulting derivatives — assuming the per-node rule
emits a constant number of new sub-expressions, true for everything except `Mul` (which
is `O(n)` per `Mul` node of fan-out `n`).

The SCOPE.md Phase 1 success criterion — `df` of a 20-term univariate polynomial in
<50 ms — is comfortably met. A 20-term polynomial has `K ≈ 60` distinct subexpressions
post-`expand` (term coefficients, monomials, `Pow` nodes, the variable). Even with
PyO3 boundary overhead and lock acquisition, this stays well under 50 ms. §6.3 commits
to the measurement.

### 5.2 Larger inputs and Jacobians

For computing the Jacobian of an `m`-output, `n`-input system (`m × n` derivatives), the
per-call cache approach is efficient when the outputs share subexpressions: differentiate
each output once with respect to each input, reusing a `DiffCache` per (output, input)
pair. Phase 2 may add a `jacobian(outputs: &[ExprId], vars: &[ExprId])` convenience
function that computes all `m × n` derivatives in one walk, sharing the `contains_symbol`
membership cache across both axes. This is a building block for Phase 2 §2.3 (matrix
operations) and Phase 3+ §3.3 (tensor algebra).

### 5.3 Parallelism within a single derivative

Phase 1 is single-threaded per call (the `&mut ExprPool` argument forces serial pool
access). For Phase 2+, the simplifier-style read-phase / write-phase split
(`designs/simplifier.md` §5.2) applies equally well: a parallel `contains_symbol`
pre-pass partitions the DAG into "depends on `var`" and "doesn't"; the writing phase then
emits derivatives only for the depends-on partition, sequentially against the single
`&mut ExprPool`. This is deferred to Phase 2 — Phase 1 single-threaded performance is
already inside the SCOPE.md target.

### 5.4 Higher-order differentiation and `dfpart`-style symbolic derivatives

REDUCE's `dfpart` package ([dfpart.red](../legacy/reduce-algebra-code-r7357-trunk/packages/misc/dfpart.red))
implements partial derivatives of *generic functions* — i.e. `df(f, x_1)` where `f` is a
named function with no body, producing the symbolic derivative `f_{(1, 0, ...)}`. The
Phase 1 differentiator handles this case via the §3.5 placeholder mechanism: `df(f(x, y), x)`
becomes `Fn(Custom("df"), [f(x, y), x])`. Phase 2 (when `depend(f, x)` declarations
arrive per SCOPE.md §2.7 user procedures) will add the dedicated `dfpart`-style notation;
the placeholder form serves as the AST hook.

### 5.5 Differentiation under the integral sign

REDUCE's `dfform_int` implements `df(int(f, x), v) → int(df(f, v), x)` under conditions.
Phase 1 emits a placeholder for `df(int(...), v)`; Phase 2 (§2.1 integration) ships the
actual rewrite. The plugin / table architecture (§3.4) accommodates this: `int` will gain
a derivative builder that knows the rewrite rule. No driver change required.

### 5.6 Integration with the simplifier

The output of `differentiate` is intentionally unsimplified (SCOPE.md §1.4). The
canonical user-facing pipeline is:

```python
result = monomix.simplify(monomix.differentiate(expr, x))
```

Per SCOPE.md §1.7 and `designs/simplifier.md` §3.3, `simplify` will collect like terms
and fold numeric coefficients in the differentiated result. The two components share no
code — they share the `ExprPool` and the `FxHashMap<ExprId, _>` cache shape, but their
algorithms are independent. This is by design: it lets the differentiator produce
predictable, structurally faithful output that the integrator (Phase 2) can pattern-match
against without contending with simplifier heuristics.

---

## 6. Testing Strategy

### 6.1 Unit tests (`cargo test`)

- **Atom rules.** `df(x, x) == 1`; `df(x, y) == 0`; `df(7, x) == 0`; `df(3/2, x) == 0`;
  `df(2.5, x) == 0`; `df("foo", x) == 0`.
- **Linearity.** `df(x + y, x) == 1`; `df(2*x + 3*y, x) == 2` (after `simplify`).
- **Leibniz.** `df(x*y, x) == y`; `df(x*y*z, x) == y*z` (after `simplify`).
- **Quotient.** `df(x/y, x) == 1/y` (after `simplify`).
- **Power, integer exponent.** `df(x^3, x) == 3*x^2` (after `simplify`).
- **Power, exponent in var.** `df(2^x, x) == 2^x * log(2)`.
- **General power.** `df(x^x, x) == x^x * (log(x) + 1)` (after `simplify`).
- **Standard functions.** Each row of the §3.4 table verified against a hand-coded
  expected expression.
- **Chain rule.** `df(sin(x^2), x) == cos(x^2) * 2*x` (after `simplify`).
- **Equation rejection.** `df(x = y, x)` returns `KernelError::DifferentiateEquation`.
- **Non-symbol var.** `df(x, x+1)` returns `KernelError::NotASymbol`.
- **Custom function placeholder.** `df(f(x), x)` (for `f` an unknown `Custom`) returns
  `Fn(Custom("df"), [f(x), x])`.

### 6.2 Property-based tests (proptest)

- **Linearity (additive).** For random `a`, `b`, `x`:
  `simplify(df(a + b, x)) == simplify(df(a, x) + df(b, x))`.
- **Linearity (scalar).** For random `a`, integer `k`, `x`:
  `simplify(df(k*a, x)) == simplify(k * df(a, x))`.
- **Leibniz (binary).** For random `a`, `b`, `x`:
  `simplify(df(a*b, x)) == simplify(df(a, x)*b + a*df(b, x))`.
- **Constant invariance.** For random `e` not containing `x`, `differentiate(e, x) == pool.zero`.
- **Idempotence on a constant.** `differentiate(differentiate(c, x), x) == pool.zero` for
  any `c` not containing `x`.
- **Determinism.** Running `differentiate(e, x)` twice on the same pool returns the same
  `ExprId`.
- **Cache equivalence.** `differentiate_fresh(e, x) == differentiate(&mut DiffCache::new(), e, x)`.

### 6.3 Benchmarks (criterion)

- `df(p, x)` where `p` is the 20-term univariate polynomial `(x+1)^20` post-`expand`.
  **Target: <50 ms wall-clock from Python (boundary included), <20 ms in Rust.**
- `df(sin(cos(tan(exp(log(sqrt(x)))))), x)` — chain rule depth-6.
  **Target: <100 µs in Rust.**
- `df(f, x)` where `f` is a fully expanded Vandermonde-like product
  `prod((x - aᵢ) for i in 1..=10)`.
  **Target: <10 ms in Rust.**
- Memory: `DiffCache` size at the end of the 20-term polynomial benchmark.
  **Target: <1 KB.**

### 6.4 Fuzz testing (cargo-fuzz)

- Feed random byte sequences to the parser → if a valid expression results, run
  `differentiate(expr, x)` → assert no panics and no invariant violations on the result
  (interned atoms only, normalized rationals, no `Eq` in output, no infinite recursion
  by way of a 1 s timeout).
- Run for ≥1 hour before each release per SCOPE.md success criteria.

### 6.5 Golden-corpus tests against legacy REDUCE

The §0.7(d) layer compares Phase 1 output against the legacy `.rlg` files. For
differentiation specifically:

- Curate a 50-example subset from `packages/alg/alg.tst` and `packages/poly/poly.tst`
  (per the §1 success criterion's "curated 50-example textbook suite") that exercises
  every Phase 1 rule.
- Each example is `(expr_string, var, expected_after_simplify)`. The pipeline is:
  parse → `differentiate` → `simplify` → format → string-compare against the captured
  `.rlg` line.
- Document divergences (e.g. coefficient ordering differences caused by the DAG's
  canonical sort) in a table next to the corpus, similar to the §3.4 table in
  `designs/simplifier.md`.

### 6.6 Cross-component invariants (with the simplifier)

The simplifier's property tests already include `df` linearity (SCOPE.md §1.12). Two
additional cross-component invariants live with this module:

- **Simplifier preserves correctness of `df`.** For random `e`, `x`:
  `simplify(df(e, x))` evaluated numerically at a random point matches `df(e, x)`
  evaluated at the same point. Catches bugs where simplification rewrites a derivative
  in a way that changes its value (e.g. an over-aggressive `(x^a)^b` rewrite from
  `designs/simplifier.md` §3.4 firing on a derivative output).
- **`evaluate_numeric ∘ df` equals numerical-difference quotient** (within tolerance)
  for random elementary expressions — a numerical sanity check that catches sign errors
  and missing chain-rule factors. Hypothesis-driven, with tolerance scaled to the
  expression's expected magnitude.

---

## 7. Action Items

### Phase 1 — Core implementation
1. [ ] Create `crates/monomix-kernel/src/diff/` module skeleton (`mod.rs`, `driver.rs`,
   `arith.rs`, `functions.rs`, `table.rs`, `plugin.rs`, `tests.rs`)
2. [ ] Implement `differentiate`, `differentiate_fresh`, and `DiffCache` per §2.1
3. [ ] Implement `diff_node` driver with per-variant dispatch (§3.1)
4. [ ] Implement `diff_mul`, `diff_div`, `diff_pow` arithmetic rules (§3.3)
5. [ ] Implement built-in derivative `table.rs` for `Sin`, `Cos`, `Tan`, `Exp`, `Log`,
   `Sqrt`, `Asin`, `Acos`, `Atan` (§3.4)
6. [ ] Implement `diff_fn` chain-rule applier with `du == pool.zero` short-circuit (§3.4)
7. [ ] Implement `symbolic_df_placeholder` for unknown functions (§3.5)
8. [ ] Define `KernelError::DifferentiateEquation` and `KernelError::NotASymbol` and
   wire them through the PyO3 boundary to `monomix.MonomixError` subclasses
8a. [ ] Add `ExprPool::func_named(name: &str, args: Vec<ExprId>) -> ExprId` accessor in
    the expression-dag module so this module can construct `Fn(Custom, ...)` placeholders
    without exposing `InternedStr` (§3.5)

### Phase 1 — Verification
9. [ ] Unit tests for §6.1 — every rule with at least one example
10. [ ] Proptest suite for §6.2 — linearity, Leibniz, constant invariance
11. [ ] Criterion benchmarks for §6.3 — confirm <50 ms on 20-term polynomial via PyO3
12. [ ] cargo-fuzz target for §6.4 — ≥1 hour clean before v0.1.0
13. [ ] Curate 50-example golden corpus from `alg.tst`/`poly.tst` per §6.5
14. [ ] Cross-component property test (`simplify ∘ differentiate` numerical check) per §6.6

### Phase 2 — Follow-ups (deferred)
15. [ ] Add `jacobian(outputs: &[ExprId], vars: &[ExprId])` building on `DiffCache` (§5.2)
16. [ ] Read-phase / write-phase split for parallel diff (§5.3) — adopt simplifier pattern
17. [ ] Differentiation under the integral sign (`df(int(f, x), v) → int(df(f, v), x)`)
   when integration ships in Phase 2 §2.1 — implement as a `table.rs` row keyed on
   `FnTag::Custom("int")` (or a dedicated `FnTag::Int` if the tag set expands)
18. [ ] `dfpart`-style symbolic partial derivatives once `depend()` declarations land in
   Phase 2 §2.7
19. [ ] Mixed-partial commutation rule (`commutedf`-equivalent) in the Phase 2
   advanced simplifier (§3.6 caveat)
20. [ ] Re-evaluate session-scoped diff cache once MCP usage data is available (§4.3)
