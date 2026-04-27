# Simplifier — System Design

**Component:** `monomix-kernel::simplify`
**Status:** Design phase
**Date:** 2026-04-26
**References:** SCOPE.md §1.7, §1.5, §1.4, §0.7; ADR-0001; ADR-0002; `designs/expression-dag.md`; `designs/parser.md`

---

## 1. Requirements

### 1.1 Functional requirements

The simplifier transforms an `ExprId` into a structurally smaller (or canonically nicer)
`ExprId` representing the same mathematical value over the reals. It is the algebraic core
that makes user-facing operations (`simplify`, post-`expand`, post-`solve`) feel correct
rather than literal.

It must support every transformation listed in SCOPE.md §1.7:

- **Numeric folding.** Constant arithmetic on `Add`, `Mul`, `Pow`, `Neg`, `Div` children
  is performed exactly: `2 + 3 → 5`, `1/2 + 1/3 → 5/6`, `(-2)^3 → -8`. Symbolic rationals
  are produced; floats appear only if a float was already present (SCOPE.md §1.1).
- **Like-term collection.** `x + x + 1 → 2*x + 1`; `2*x + 3*x → 5*x`; coefficients combine
  exactly (rational arithmetic, never lossy float folding).
- **Common-factor cancellation.** `x^2 / x → x`; `(x^2 + x) / x → x + 1`. Implemented on
  top of the §1.5 univariate polynomial division engine; the simplifier orchestrates the
  call but does not duplicate the algorithm.
- **Power consolidation.** `x * x → x^2`; `x^a * x^b → x^(a+b)`; `(x^a)^b → x^(a*b)` when
  `a, b` are integers or rationals (the integer/rational case is unconditional; float
  exponents are not consolidated to avoid `(-1)^(1/2)` traps).
- **Trivial identity elimination.** `0 + x → x`, `1 * x → x`, `x^1 → x`, `x^0 → 1`,
  `0 * x → 0`, `x - x → 0`. **These are guaranteed by `ExprPool` interning at construction
  time** (see `designs/expression-dag.md` §3.1, Invariant 5). The simplifier does not
  re-implement them; it relies on the pool returning normalized handles.
- **Pythagorean identity.** `sin(u)^2 + cos(u)^2 → 1` for any sub-expression `u`, including
  when the pair appears alongside other addends: `a + sin(u)^2 + b + cos(u)^2 → a + b + 1`.
- **Idempotence.** `simplify(simplify(e)) ≡ simplify(e)` structurally (same `ExprId`).
  Listed as a property-based invariant in SCOPE.md §1.12.

### 1.2 Non-functional requirements

| Requirement | Target | Rationale |
|-------------|--------|-----------|
| Termination on any input | Always — no infinite-rewrite loops | Correctness; verified by fuzz |
| Idempotence | `simplify ∘ simplify = simplify` | SCOPE.md §1.12 property test |
| Determinism | Same input ⇒ same output `ExprId` across runs | Tests + cache reproducibility |
| Latency (50-term sum) | <100 ms wall-clock from Python | SCOPE.md §1, Phase 1 success criterion |
| No `unsafe` | Required | Kernel rule (ADR-0002) |
| `Send + Sync` | Required | MCP server concurrency (ADR-0002, §0.5) |
| GIL release | Yes, for inputs >500 nodes | SCOPE.md §0.5 — release if expected >1 ms |
| Plugin extensibility | Read-only rule registration in Phase 1 | SCOPE.md §1.10; full read/write in Phase 2 §2.6 |

### 1.3 Constraints

- The simplifier reads expressions via `&ExprPool` and creates new ones via
  `&mut ExprPool::*` constructors. **It never constructs `ExprNode` directly.** This
  preserves the eager-normalization invariants of §3.1 of the expression DAG design.
- The simplifier is **stateless across calls** — all working state is on the stack or in
  caller-owned scratch buffers. No global rule database; no thread-local interning.
- The simplifier **must not mutate inputs in place** — interned nodes are immutable.
- The simplifier is **bounded in Phase 1**: only the transformations enumerated in §1.1
  are performed. Out of scope: trigonometric identities beyond Pythagorean
  (`sin(2x) = 2 sin(x) cos(x)` etc.), logarithm/exponential combination, partial-fraction
  decomposition, algebraic-number simplification, context-aware assumptions (`assume(x>0)`).
  All deferred to Phase 2 §2.6.
- **No automatic simplification post-differentiation** (SCOPE.md §1.4). The simplifier is
  invoked only when the user (or a kernel routine) explicitly requests it.

---

## 2. High-Level Design

### 2.1 Public API

```rust
/// Simplify `root` and return the canonical ExprId.
/// Idempotent: simplify(pool, simplify(pool, e)) == simplify(pool, e).
/// Never panics; runtime errors (e.g. encountered division by zero) are returned.
pub fn simplify(pool: &mut ExprPool, root: ExprId) -> Result<ExprId, KernelError>;

/// Same as `simplify`, but with a caller-owned scratch cache for amortizing cost
/// across multi-pass pipelines (e.g. simplify ∘ substitute ∘ differentiate).
/// The cache maps `ExprId → ExprId` (input → simplified) for the current pool generation.
pub fn simplify_with_cache(
    pool: &mut ExprPool,
    root: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Result<ExprId, KernelError>;
```

The pool is monotonic in Phase 1 — `ExprId`s are never reassigned to different content
(see expression DAG §5.1) — so the cache is safe to reuse across any number of
`simplify_with_cache` calls within a session. The convenience entry point
`simplify(pool, root)` allocates a fresh cache per call; long-running pipelines (e.g.
`simplify ∘ substitute ∘ simplify`) should hold one cache and pass it in. Phase 2's
generational pool (expression DAG §5.1) will require attaching a generation tag to the
cache and invalidating on generation change; the API stays the same.

### 2.2 Component diagram

```
                ExprId (root)
                     │
                     ▼
            ┌─────────────────┐
            │   driver.rs     │  bottom-up rewrite via map_bottom_up
            │  (fixed-point)  │  with per-call cache
            └────────┬────────┘
                     │ for each node, dispatch by ExprNode variant:
                     ▼
   ┌─────────────────────────────────────────────────────────┐
   │                                                         │
   ▼                ▼              ▼              ▼          ▼
┌──────┐     ┌─────────────┐  ┌───────────┐ ┌──────────┐ ┌────────┐
│numeric│     │ like_terms  │  │ powers    │ │ rational │ │pattern │
│ fold  │     │ (Add/Mul    │  │ (x*x→x^2, │ │ (Div via │ │matcher │
│       │     │  collector) │  │  x^a*x^b) │ │  poly /) │ │ (trig) │
└───┬───┘     └──────┬──────┘  └─────┬─────┘ └────┬─────┘ └───┬────┘
    │                │                │            │           │
    └────────────────┴────────────────┴────────────┴───────────┘
                     │
                     ▼
              pool.add / pool.mul / pool.pow / pool.div / ...
              (re-interned through ExprPool's normalizing constructors)
                     │
                     ▼
              new ExprId returned to driver
```

### 2.3 Module layout

```
crates/monomix-kernel/src/simplify/
├── mod.rs           — public API (`simplify`, `simplify_with_cache`), KernelError mapping
├── driver.rs        — bottom-up traversal, fixed-point loop, cache
├── numeric.rs       — exact constant folding (BigInt + Rational)
├── like_terms.rs    — Add/Mul children → (coefficient, monomial) buckets → re-emit
├── powers.rs        — x*x, x^a*x^b, (x^a)^b consolidation
├── rational.rs      — Div(p, q) → polynomial quotient + remainder
├── patterns.rs      — minimal term-rewriting engine
├── rules.rs         — built-in rule registry (currently: Pythagorean only)
└── tests.rs
```

The split mirrors the kernel convention from `parser/`: each phase has a focused module,
the driver pulls them in via dispatch, and rules-as-data live separately from the engine
that applies them so the Phase 2 generalization (SCOPE.md §2.6) is additive.

### 2.4 Single-pass vs. fixed-point

The simplifier is **bottom-up with a bounded fixed-point loop** at the root.

- **Bottom-up pass.** A single bottom-up traversal (using `map_bottom_up` from the
  expression DAG design §3.6, with the caller-owned cache) applies all of §1.1's
  transformations once to every node.
- **Fixed-point loop.** A bottom-up pass can expose new opportunities at the root (e.g.,
  collecting `sin(u)^2 + cos(u)^2` after the children have been canonicalized first). The
  driver re-runs the bottom-up pass until two successive passes yield the same `ExprId`
  for the root, with a hard cap of **3 iterations** in Phase 1.

The cap is essential: it gives a static upper bound on work, matches the depth of rule
interaction in Phase 1's narrow rule set (children-canonical → trig pair → numeric fold of
the resulting `1`), and makes regression on termination impossible without an explicit
code change. Empirically all Phase 1 transformations converge in ≤2 passes; the third
slot is headroom. If a future change pushes the bound, the test suite catches it (the
fixed-point counter is exposed in debug builds for assertions).

This structure deliberately rules out the open-ended "rewrite-until-stable" loops that
make CAS simplifiers infamous for hangs. Phase 2's general rule engine (§2.6) will
introduce a confluence-checked rule set with its own termination strategy; the Phase 1
engine does not.

### 2.5 Data flow through a single pass

```
  input ExprId
       │
       ▼
  cache hit? ─── yes ──► return cached ExprId
       │ no
       ▼
  read &ExprNode from pool
       │
       ▼
  recursively simplify children (map_bottom_up)
       │
       ▼
  rebuild with simplified children via pool constructors
   (this triggers ExprPool's eager normalizations:
    Neg(Neg(x))→x, x^0→1, x^1→x, sorted children, flattened Add/Mul)
       │
       ▼
  variant-specific dispatch:
   ┌─ Add(children)  → numeric_fold + like_terms + pythagorean_match
   │                  ↓
   ├─ Mul(children)  → numeric_fold + like_terms_mul + power_merge
   │                  ↓
   ├─ Pow(b, e)      → power_of_power, x^integer simplifications
   │                  ↓
   ├─ Div(n, d)      → rational_simplify (delegates to poly division)
   │                  ↓
   ├─ Fn(tag, args)  → no-op in Phase 1 (function bodies don't auto-evaluate)
   │                  ↓
   └─ atoms          → identity
       │
       ▼
  insert (input → result) into cache
       │
       ▼
  return result ExprId
```

Eager normalizations in `ExprPool` (e.g., `pool.pow(x, one) → x`) handle the trivial
identities listed in SCOPE.md §1.7 without any code in this module. The simplifier
focuses on the work the pool cannot do alone: arithmetic on numeric children, structural
re-grouping, and pattern-based rewriting.

---

## 3. Deep Dive

### 3.1 Driver and cache (`driver.rs`)

```rust
pub fn simplify_with_cache(
    pool: &mut ExprPool,
    root: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Result<ExprId, KernelError> {
    const MAX_ITERS: u8 = 3;
    let mut current = root;
    for _ in 0..MAX_ITERS {
        let next = simplify_one_pass(pool, current, cache)?;
        if next == current {
            return Ok(current);
        }
        current = next;
    }
    // The cap is a guarantee, not an error — convergence is asserted in tests.
    // If we exit here it means a bug in a rule allowed cycling; surface it.
    debug_assert_eq!(current, simplify_one_pass(pool, current, cache)?,
        "simplifier failed to converge in {MAX_ITERS} iterations");
    Ok(current)
}

fn simplify_one_pass(
    pool: &mut ExprPool,
    root: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Result<ExprId, KernelError> {
    map_bottom_up(pool, root, cache, &mut |pool, id| simplify_node(pool, id))
}
```

`simplify_node` is the per-variant dispatcher. Because `map_bottom_up` already memoizes on
`ExprId`, shared subexpressions in the DAG are simplified exactly once per pass — this is
the dividend of hash-consing. A polynomial expansion that produces `(x+1)^10` flattened
into ~1000 internal monomials but with only ~40 unique subnodes simplifies in 40 unit
calls, not 1000.

**Cache invalidation.** The cache is keyed on the *input* `ExprId`; the value is the
simplified `ExprId`. Both are valid as long as the pool's `ExprId → ExprNode` mapping
is monotonic (Phase 1 guarantee — see expression DAG design §5.1). A future generational
pool would attach a generation counter to the cache and invalidate on generation change.

### 3.2 Numeric folding (`numeric.rs`)

Exact arithmetic on `SmallInt`, `BigInt`, `Rational`, and (when present) `Float` children:

```rust
/// Partition children into numeric and symbolic. Fold the numeric subset into a single
/// rational value (or float, if any child is a float — the float taints the partition).
/// Returns (folded_value_id_or_None, leftover_symbolic_children).
fn fold_addends(pool: &mut ExprPool, children: &[ExprId])
    -> (Option<ExprId>, SmallVec<[ExprId; 8]>);

fn fold_factors(pool: &mut ExprPool, children: &[ExprId])
    -> (Option<ExprId>, SmallVec<[ExprId; 8]>);
```

Rules:

- **Integer + Integer → Integer.** Performed via `BigInt`; the result is routed through
  `pool.integer()` which selects `SmallInt` vs `BigInt` automatically.
- **Rational + Rational / Integer + Rational → Rational** in lowest terms. The pool's
  `rational()` constructor enforces normalization (Invariant 3 in expression DAG §3.1).
- **Float taint.** If any operand is a `Float`, the whole numeric partition collapses to
  a float. This matches SCOPE.md §1.1's "the two are not silently mixed" rule — the
  simplifier never *introduces* a float, but if the user wrote one, all numerics in the
  same Add/Mul fold into the float. This is the only place float arithmetic appears in
  the simplifier; symbolic rationals are otherwise the default representation.
- **Identities from folding.** If folding produces `0` in an `Add`, it is omitted (the
  pool's `add()` already drops zero children, but we may finish with a singleton list and
  must return that single child rather than wrap it in a 1-ary `Add`). Likewise `1` in
  `Mul` and `0` in `Mul` (the latter collapses the entire Mul to zero — handled before
  re-emission).

`Pow(numeric, integer)` is folded directly: `(2)^3 → 8`, `(1/2)^(-2) → 4`. `Pow(float, _)`
and `Pow(_, float)` are *not* folded — the simplifier does not introduce floats and does
not touch transcendental identities. `Pow(_, rational)` with a non-integer exponent is
preserved symbolically (e.g. `4^(1/2)` stays as-is; the user can request numeric
evaluation explicitly per SCOPE.md §1.8).

### 3.3 Like-term collection (`like_terms.rs`)

This is the central simplification step for `Add` and the structural mirror for `Mul`.

**Add: coefficient/monomial decomposition.**

Each child of `Add` is decomposed into `(coefficient, monomial)`:

- `x` → `(1, x)`
- `2*x` → `(2, x)`
- `(-1) * y` → `(-1, y)`
- `(3/2) * x * y` → `(3/2, x*y)`
- `5` (pure numeric) → `(5, 1)` — handled by §3.2 as a separate path
- `sin(x)` → `(1, sin(x))` — opaque function applications are atoms for like-term purposes

The decomposition algorithm is local — it inspects the immediate `Mul` children of an
`Add` child and partitions them into "rational coefficient" vs "everything else":

```rust
fn split_coefficient(pool: &ExprPool, term: ExprId) -> (Coeff, ExprId) {
    match pool.get(term) {
        ExprNode::Mul(children) => {
            // First numeric child is the coefficient (canonical sort puts numerics first).
            // Rebuild the monomial from the remaining children.
            // ...
        }
        ExprNode::Neg(inner) => {
            let (c, m) = split_coefficient(pool, *inner);
            (-c, m)
        }
        ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_) => {
            (term_as_coeff(pool, term), pool.one)
        }
        _ => (Coeff::one(), term),
    }
}
```

Children of `Mul` are stored in canonical sort order (expression DAG Invariant 4); numeric
atoms sort lowest by construction (the `LocalExprId(u32)` of the pre-interned `zero`,
`one`, `minus_one` are the smallest indices in the arena, and other small integers
follow). This means the numeric coefficient — when present — is always the first child of
the `Mul`. Decomposition is therefore O(1) inspection, not a search.

**Bucket and re-emit.**

```
input:  Add([x, x, 1, sin(y), 3*x, sin(y)])
        │
        ▼
decompose each addend → list of (coeff, monomial):
        (1, x), (1, x), (1, 1), (1, sin(y)), (3, x), (1, sin(y))
        │
        ▼
group by monomial (FxHashMap<ExprId, Coeff>):
        x        → 1 + 1 + 3 = 5
        1        → 1
        sin(y)   → 1 + 1 = 2
        │
        ▼
re-emit:  pool.add([pool.mul([5, x]), 1, pool.mul([2, sin(y)])])
        which the Add constructor sorts and flattens canonically.
```

The bucket is a `FxHashMap<ExprId, Coeff>` keyed on the monomial `ExprId`. Because two
mathematically-equal monomials always have the same `ExprId` (hash-consing — expression
DAG Invariant 1), the map collapses them automatically. **No structural comparison is
needed at bucket-insertion time** — this is the key payoff of hash-consing for the
simplifier and the reason like-term collection is O(n) rather than O(n²).

`Coeff` is `enum Coeff { Int(i64), Big(Box<BigInt>), Rat(Box<(BigInt, BigInt)>) }` — the
same i64 fast-path / heap-fallback split that `ExprNode` uses for atoms. After bucketing,
each (monomial, coeff) pair is re-encoded into an `ExprId` via the pool: zero coefficients
are dropped, unit coefficients yield the bare monomial, otherwise `pool.mul([coeff, mon])`.

**Mul: like-base/exponent collection** is structurally analogous. Each factor is
decomposed into `(base, exponent)`:

- `x` → `(x, 1)`
- `x^3` → `(x, 3)`
- `x^(1/2)` → `(x, 1/2)`
- `2^x` → `(2, x)` — symbolic exponent; bucketed as a base of `2` with that exponent

Bucketed by base; exponents are summed (using the same numeric folding from §3.2). This
produces `x*x → x^2` and `x^a * x^b → x^(a+b)`. See §3.4 for the integer-exponent caveat
on `(x^a)^b`.

**Why a hashmap, not a sort.** Children of `Add` and `Mul` are already sorted by `ExprId`
(expression DAG Invariant 4), but the bucket keys here are *monomials*, not the original
children — distinct children can map to the same monomial (`x` and `2*x` both have
monomial `x`). Sorting after decomposition would be O(n log n); a hashmap is O(n) and
matches the hot-path budget.

### 3.4 Power consolidation (`powers.rs`)

Three transformations beyond what `pool.pow()` already does at intern time:

1. **`x * x → x^2`** falls out of §3.3's Mul like-base bucketing.
2. **`x^a * x^b → x^(a+b)`** — same path, with non-trivial exponents.
3. **`(x^a)^b → x^(a*b)`** — applied at `Pow` simplification time. Implemented as:

```rust
fn simplify_pow(pool: &mut ExprPool, base: ExprId, exp: ExprId) -> ExprId {
    match pool.get(base) {
        ExprNode::Pow(inner_base, inner_exp) => {
            let inner_base = *inner_base;
            let inner_exp = *inner_exp;
            // Only consolidate if both exponents are integer or rational.
            // (x^(1/2))^2 = x is sound; (x^a)^b = x^(a*b) is unsound for negative x
            // and float exponents because of branch cuts. We reject float to stay safe.
            if is_int_or_rational(pool, inner_exp) && is_int_or_rational(pool, exp) {
                let combined = pool.mul(vec![inner_exp, exp]);
                let combined = simplify(pool, combined)?; // fold the product
                return pool.pow(inner_base, combined);
            }
            pool.pow(base, exp)
        }
        _ => pool.pow(base, exp),
    }
}
```

The integer/rational guard is deliberate. `(x^2)^(1/2)` is *not* `x` over the reals —
it's `|x|`. SCOPE.md §1.7 lists the rule unconditionally, but in Phase 1 we restrict it
to cases where the algebraic identity is sound for any real `x`:

| `inner_exp` | `outer_exp` | Sound for all real `x`? | Apply? |
|-------------|-------------|--------------------------|--------|
| integer | integer | yes (always) | yes |
| integer | rational | only if outer × inner gives integer | conservative: yes only when product is integer |
| rational | integer | yes if product is integer; otherwise positive-`x` only | conservative: yes only when product is integer |
| rational | rational | rarely sound | conservative: no |
| anything | float | branch cut hazard | no |
| float | anything | branch cut hazard | no |

The "conservative: yes only when product is integer" cases (e.g. `(x^(1/2))^2 = x^1 = x`)
are sound because the resulting identity matches at every real `x` where the original
expression is defined.

### 3.5 Rational expression simplification (`rational.rs`)

`Div(p, q)` simplification is a thin orchestrator over the §1.5 univariate polynomial
division engine. The simplifier does **not** re-implement polynomial GCD / division — it
delegates and re-encodes the result.

```rust
fn simplify_div(pool: &mut ExprPool, num: ExprId, den: ExprId) -> Result<ExprId, KernelError> {
    // Quick path: numeric / numeric → exact rational (or float, if either operand is a
    // float). Note that `pool.div(integer, integer)` does *not* fold to `Rational` at
    // intern time (expression DAG §3.1 Invariant 5 lists only `Div(a, 1) → a`). The
    // simplifier extracts the operand values and routes through `pool.rational()` for
    // exact division, or `pool.float()` if a float taints the operation.
    if pool.is_numeric(num) && pool.is_numeric(den) {
        return numeric::divide(pool, num, den);  // internal helper, not a pool method
    }

    // Try to view num and den as univariate polynomials in the common variable.
    let var = match common_univariate(pool, num, den) {
        Some(v) => v,
        None => return Ok(pool.div(num, den)),  // give up; multivariate is Phase 2
    };

    let (num_poly, den_poly) = (
        polynomial::view(pool, num, var)?,
        polynomial::view(pool, den, var)?,
    );

    // Polynomial division with remainder.
    let (quotient, remainder) = polynomial::divide(pool, &num_poly, &den_poly)?;

    if polynomial::is_zero(&remainder) {
        // Exact division: x^2 / x → x.
        return Ok(polynomial::to_expr(pool, &quotient));
    }

    // Inexact: x^2 + 1 / x → quotient + remainder/den. Only emit if the remainder is
    // simpler than re-emitting the original Div (e.g. lower degree). Otherwise leave
    // the original Div in place so the user-facing form does not become noisier.
    if polynomial::degree(&remainder) < polynomial::degree(&den_poly) - 1 {
        let quot_id = polynomial::to_expr(pool, &quotient);
        let rem_id  = polynomial::to_expr(pool, &remainder);
        let rem_div = pool.div(rem_id, den);
        return Ok(pool.add(vec![quot_id, rem_div]));
    }

    Ok(pool.div(num, den))
}
```

**Why not always split into quotient + remainder/den.** Because the user-facing intent of
`simplify` is "make it look better." Over-eager splitting turns `(x+1)/(x-1)` into
`1 + 2/(x-1)` — mathematically correct but visually noisier for a typical user. Phase 2's
advanced simplifier (§2.6) can offer this as a separate `apart()` operation; in Phase 1,
the rule is "cancel only when the remainder is zero, and otherwise leave the form alone."

**Multivariate division** is Phase 2 (§2.5). When `num` and `den` involve multiple
variables, the simplifier returns the original `Div` unchanged. A diagnostic is *not*
emitted — this is a feature limitation, not an error.

**Division by zero detection.** If `den` simplifies to a recognized zero (`pool.is_zero`),
`simplify_div` returns `KernelError::DivisionByZero`. Symbolic denominators that *might*
be zero (e.g. `1 / (x - x)` after the inner simplification produces zero) trigger the same
error. The error carries the original parse span when available (via the parser's
`SpanMap`).

### 3.6 Pattern matching (`patterns.rs`, `rules.rs`)

The Phase 1 pattern matcher exists for one purpose: the Pythagorean identity
`sin(u)^2 + cos(u)^2 → 1`. SCOPE.md §1.7 calls for it to fire even when the pair is
embedded in a larger sum: `a + sin(u)^2 + b + cos(u)^2 → a + b + 1`. This requires
matching across `Add` siblings, which is more than a simple top-down structural match.

**Rule representation.**

```rust
/// A Phase 1 rule pattern. Variables (`MetaVar`) bind sub-expressions during matching.
/// Phase 2 generalizes this to user-supplied rules; Phase 1 only ships built-ins.
#[derive(Clone)]
pub enum Pattern {
    /// Matches any expression and binds it to a slot.
    Var(MetaVar),
    /// Matches a specific expression literally (e.g. the integer 2).
    Lit(ExprId),
    /// Matches a function application with this tag and these argument patterns.
    Fn(FnTag, Vec<Pattern>),
    /// Matches a Pow(base_pattern, exp_pattern).
    Pow(Box<Pattern>, Box<Pattern>),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MetaVar(u8);  // small fixed pool — Phase 1 needs ≤8 distinct binders

/// Matches a multiset of children (used for Add/Mul rules). Each `Pattern` consumes one
/// element of the children list; `rest_var` (optional) captures the leftover children
/// as a sub-multiset, enabling the "embedded in a larger sum" rule.
pub struct CommutativePattern {
    pub elements: Vec<Pattern>,
    pub rest_var: Option<MetaVar>,
}

pub struct Rule {
    pub kind: RuleKind,
    pub matches: fn(pool: &ExprPool, root: ExprId, env: &mut MatchEnv) -> bool,
    pub rewrite: fn(pool: &mut ExprPool, env: &MatchEnv) -> Result<ExprId, KernelError>,
}

pub enum RuleKind {
    /// Applies to a single node when it has the matching shape.
    PointWise,
    /// Applies to children of an Add/Mul (commutative multiset match).
    AddCommutative,
    MulCommutative,
}
```

**Pythagorean rule.**

```rust
pub fn pythagorean_rule() -> Rule {
    Rule {
        kind: RuleKind::AddCommutative,
        matches: |pool, root, env| {
            // root is an Add. We need two siblings that match sin(u)^2 and cos(u)^2
            // with the same `u`.
            let ExprNode::Add(children) = pool.get(root) else { return false };
            // Try every (i, j) pair; take the first match. Phase 1 N is small in practice.
            for i in 0..children.len() {
                for j in (i+1)..children.len() {
                    if let Some(u) = match_sin2_cos2_pair(pool, children[i], children[j]) {
                        env.bind(MetaVar(0), u);
                        env.consume(i);
                        env.consume(j);
                        return true;
                    }
                }
            }
            false
        },
        rewrite: |pool, env| {
            // Replace the consumed pair with the literal 1; leave other children alone.
            let one = pool.one;
            let mut rest: Vec<ExprId> = env.unconsumed_children().to_vec();
            rest.push(one);
            Ok(pool.add(rest))
        },
    }
}

fn match_sin2_cos2_pair(pool: &ExprPool, a: ExprId, b: ExprId) -> Option<ExprId> {
    let (sin_arg, cos_arg) = (extract_sin_squared(pool, a)?, extract_cos_squared(pool, b)?);
    if sin_arg == cos_arg { return Some(sin_arg); }
    // Try the swap.
    let (sin_arg, cos_arg) = (extract_sin_squared(pool, b)?, extract_cos_squared(pool, a)?);
    if sin_arg == cos_arg { return Some(sin_arg); }
    None
}
```

`extract_sin_squared(pool, e)` returns `Some(u)` iff `e` is `Pow(Fn(Sin, [u]), 2)` (where
`2` is the `SmallInt`). Equality between two argument expressions `u₁` and `u₂` is `u₁ ==
u₂` — a single `ExprId` comparison, courtesy of hash-consing.

**Why O(n²) on Add children is acceptable in Phase 1.** `n` here is the number of
top-level addends in a single `Add` node, not the size of the expression tree. Even
post-`expand`, this is typically ≤ a few dozen for human-written input. The pessimistic
case (1000-term polynomial) is excluded by the polynomial path in §3.5 — the simplifier
recognizes that no `sin`/`cos` subterms exist via a fast pre-scan and skips the
Pythagorean rule entirely.

**Pre-scan optimization.** Before invoking the rule, the driver checks
`bucket.contains_function(FnTag::Sin) && bucket.contains_function(FnTag::Cos)` (a single
bit-set lookup populated during like-term collection). If either is absent the rule is
skipped — this keeps polynomial workloads fast.

**Rule registration.**

```rust
pub struct RuleRegistry {
    pointwise: Vec<Rule>,
    add_rules: Vec<Rule>,
    mul_rules: Vec<Rule>,
}

impl Default for RuleRegistry {
    fn default() -> Self {
        Self {
            pointwise: vec![],
            add_rules: vec![pythagorean_rule()],
            mul_rules: vec![],
        }
    }
}
```

The registry is owned by the `Session` (Python-side, per SCOPE.md §1.3) and passed by
reference into `simplify`. **In Phase 1, plugins can register read-only rules** at
`Session` construction (per SCOPE.md §1.10's "register a rewrite rule that the simplifier
will consider"). Phase 2 §2.6 generalizes this to a user-writable rule database with
confluence checking.

### 3.7 Termination and confluence

**Termination.** Each transformation is monotone in the structural metric below; combined
with the §2.4 fixed-point cap of 3 iterations, this gives a hard upper bound on work:

```
metric(e) = (count of non-canonical sub-nodes,    // Add/Mul that could re-flatten,
                                                  // Pow that could re-consolidate, etc.
             count of like-term-collapsible pairs,
             count of Pythagorean pairs)
```

Each rule strictly decreases at least one component without increasing the others.
`simplify_node` cannot revisit the same node within a single pass (the bottom-up cache
guarantees one visit per `ExprId`), so a single pass terminates trivially. Across passes,
the metric strictly decreases until it reaches `(0,0,0)`, at which point the next pass
returns the same `ExprId` and the loop exits.

**Confluence.** In Phase 1 the rule set is small enough that confluence is verifiable by
inspection — no two rules apply to overlapping shapes in a way that would produce
different normal forms. The `proptest` suite (§6.2) verifies this empirically:
`simplify(simplify(e)) == simplify(e)` for randomly generated inputs.

Phase 2's general rule engine cannot rely on inspection-level confluence; the rule
database will need a Knuth-Bendix style analysis or a fixed rule-application order. That's
deferred along with the engine itself.

### 3.8 PyO3 boundary

```rust
// In monomix-py/src/lib.rs
#[pyfunction]
fn simplify(py: Python<'_>, expr: &PyExpr) -> PyResult<PyExpr> {
    let pool_handle = expr.pool.clone();          // Arc clone, cheap
    let id          = expr.id;
    let new_id = py.allow_threads(|| {            // GIL released
        let mut pool = pool_handle.write();       // exclusive write lock for interning
        monomix_kernel::simplify(&mut pool, id)
    })?;
    Ok(PyExpr { pool: pool_handle, id: new_id })
}
```

The GIL is released for the duration of `simplify()`. The pool is held under a write lock
because the simplifier interns new nodes; concurrent simplifies on the same pool serialize
on the lock. Phase 2's read-phase / write-phase split (expression DAG §5.2) is the path to
parallel simplification within a single request.

For inputs smaller than a configurable threshold (default: 500 nodes — measured by a
linear `pool.len()` comparison or a cheap pre-walk count), the GIL is *not* released
because the Python-side overhead of `allow_threads` exceeds the work done. The threshold
is tuned per the SCOPE.md §1.12 benchmark target (<500 ns boundary overhead per call).

### 3.9 Error handling

| Error | Source | Handling |
|-------|--------|----------|
| `DivisionByZero` | `Div(p, 0)` after simplification | Return `KernelError::DivisionByZero` with span if available |
| `Overflow` | `BigInt` arithmetic exhausting memory | Propagate `KernelError::Overflow` (very rare; only on truly enormous coefficients) |
| `NumericNaN` | Float arithmetic produces NaN | Preserve the NaN in the result; do not raise (NaN is a valid float value) |
| `RuleEvalError` | A plugin rule's `rewrite` fn returns Err | Propagate; the offending rule is named in the error |

The simplifier never panics. Internal invariant violations (e.g. a like-term bucket out of
sync) are caught by `debug_assert!` in debug builds and become benign no-ops in release
builds — the simplifier returns the input unchanged rather than crashing on user-visible
errors.

---

## 4. Trade-off Analysis

### 4.1 Bottom-up rewriting vs. top-down or e-graphs

**Chosen: bottom-up rewriting with a bounded fixed-point loop.**

| Dimension | Bottom-up + fixed-point | Top-down (sympy-style) | E-graph (egg / equality saturation) |
|-----------|------------------------|------------------------|-------------------------------------|
| Termination guarantee | Hard cap on iterations | Hard, but fewer rule interactions | Saturation can blow up |
| Confluence handling | Inspection-level (Phase 1) | Rule-order-dependent | Saturation finds canonical form |
| Implementation complexity | Low | Low | High (union-find + extraction) |
| Runtime per call | O(n) per pass × constant passes | O(n) | O(n × rules × saturation depth) |
| Memory | Reuses ExprPool, no extra DAG | Reuses ExprPool | Builds parallel e-graph |
| Plugin ergonomics | Rule = function pair | Rule = function pair | Pattern → rewrite literal |

**Why bottom-up wins for Phase 1.** The rule set is tiny and confluence is trivially
verifiable. E-graphs are the right tool when you have hundreds of competing rewrite rules
and need an optimal extraction; for Phase 1's fixed rule set, they're heavy machinery for
no payoff. Top-down has its place but produces non-canonical results when child
simplifications enable parent-level rewrites — exactly the case for Pythagorean (children
must canonicalize first).

**Revisit trigger.** Phase 2 §2.6 introduces a general rule engine. If the rule database
grows past ~30 rules with non-obvious interactions, evaluate `egg` (the Rust e-graph
crate) as the engine — its Apache 2.0 license is compatible, and its model maps cleanly
onto the existing `ExprPool` (e-classes are sets of `ExprId`s).

### 4.2 Like-term bucket: HashMap vs. sort-and-scan

**Chosen: `FxHashMap<ExprId, Coeff>` keyed on monomial `ExprId`.**

| Approach | Time | Space | Notes |
|----------|------|-------|-------|
| HashMap on monomial ExprId | O(n) | O(n) | Hash-cons makes the key directly comparable |
| Sort by monomial then scan | O(n log n) | O(n) | Requires a comparator — `ExprId` ordering is arbitrary, so we'd sort by content, which is expensive |
| Linear scan with structural compare | O(n²) | O(1) | Only acceptable for tiny n |

The HashMap path leans on hash-consing: the monomial `ExprId`s are *already* deduplicated
by content, so `==` on `ExprId` is content equality. No structural compare is ever
needed. This is a load-bearing payoff of the expression DAG design — without it, the
simplifier would need its own hashing or sorting on monomial structure.

### 4.3 Eager normalization in `ExprPool` vs. in the simplifier

**Chosen: eager in the pool (already established by expression DAG §3.1) + structural rewrites in the simplifier.**

The pool handles `Neg(Neg(x)) → x`, `x^0 → 1`, `x^1 → x`, `Div(a, 1) → a`, child sorting,
flattening of `Add`/`Mul`. The simplifier handles arithmetic, bucketing, and rule-based
matching.

**Why this split.** The pool's normalizations are unconditional and free (they make
interning produce a unique handle). The simplifier's transformations are conditional and
expensive (require pool mutation, traversal, hashing). Putting "trivial" rewrites in the
pool means they happen during *every* expression construction, including parser output —
not just when `simplify` is called. By the time a user-built expression reaches
`simplify`, the trivial cases are already handled and the simplifier can focus on the
work that requires inspection of multiple children.

A purely-simplifier model would require parser-built expressions to be `simplify`d before
display (otherwise `--x` shows up in the REPL as `Neg(Neg(x))`). The pool-eager model
makes display-correctness independent of explicit simplification calls.

### 4.4 Polynomial division reuse vs. ad-hoc cancellation

**Chosen: delegate to the §1.5 polynomial division engine; do not duplicate.**

The simplifier could implement local cancellation (e.g., scan numerator monomials, scan
denominator monomials, divide common factors). This would handle simple cases (`x^2/x`)
without invoking the polynomial engine. But:

- The polynomial engine *already* does this and more (`(x^2 + x)/x → x + 1`).
- Duplicating logic invites drift between the two paths.
- The performance gap is small for simple cases (the polynomial engine has a fast path
  for monomial inputs).

The simplifier's job is to *recognize* that a `Div` node is a candidate for polynomial
simplification and to translate the result back into expression nodes. The engine does
the algebra.

### 4.5 Pattern-matching engine vs. hand-coded rule per identity

**Chosen: minimal pattern-matching engine (`patterns.rs`).**

Phase 1 has exactly one non-trivial pattern (Pythagorean). Hand-coding it would be ~30
lines and would let us defer the pattern engine to Phase 2 §2.6.

Reason for the engine: the pattern engine *is* the contract that Phase 2's general rule
engine extends. Designing the rule shape (`Rule { kind, matches, rewrite }`) and the
match environment (`MatchEnv` with `bind/consume/unconsumed_children`) in Phase 1 — even
with a single rule consuming it — exercises the contract while it's still cheap to change.
By Phase 2 the contract is documented and tested; adding rules is purely additive.

The cost is the ~150 lines of `patterns.rs` and the `Rule` struct, which is offset by
removing the temptation to inline more rules ad-hoc as the rule set grows.

---

## 5. Scale, Limits, and Future Work

### 5.1 Phase 2: General rule database (SCOPE.md §2.6)

Phase 2 §2.6 generalizes the pattern matcher into a user-extensible rule database. The
groundwork in Phase 1:

- `Rule`, `Pattern`, `MetaVar`, `MatchEnv` types are already defined.
- `RuleRegistry` is owned by the Session, separate from the engine.
- The engine's `simplify_node` dispatcher is variant-keyed, so adding new `RuleKind`
  variants (e.g. `RuleKind::Conditional` with a guard predicate) is non-breaking.

Phase 2 work concentrates on:

- **Surface syntax.** A REDUCE-style `let sin(~x)^2 = 1 - cos(~x)^2;` (per the parser
  design §5.1) parses into a `Rule`. Plugin authors can register rules from Python via a
  builder API that produces the same `Rule` struct.
- **Confluence.** A static check or a fixed application order to keep the simplifier
  deterministic in the face of overlapping rules.
- **Conditional rules.** Predicates (`assume(x > 0)` style) that gate rule application.
  Out of scope for Phase 1 entirely (SCOPE.md §1.7's "no context-aware assumptions").

### 5.2 Parallel simplification within a request

The Phase 1 simplifier holds a write lock on the pool for the duration of a call. Phase
2 wants per-request parallelism. Two options:

- **Read-phase / write-phase split** (expression DAG §5.2): Phase A walks the DAG under a
  read lock, computing per-`ExprId` rewrite descriptors. Phase B applies them under a
  write lock in a single pass. Compatible with the existing `RwLock<ExprPool>`.
- **Pure functional kernel.** The simplifier itself is referentially transparent — same
  input `ExprId` always produces the same output `ExprId`. This makes parallel `fork-join`
  simplification of independent subtrees natural: each thread holds a read lock during
  decomposition, releases it for child simplifies, re-acquires write lock for re-emission.

The first option is preferred because it's a pattern shared with other kernel modules
(differentiator, polynomial division) and amortizes the lock cycling across them.

### 5.3 Result caching across sessions / machines

For Phase 2's MCP cache (SCOPE.md §2.8), the simplifier's input/output `ExprId` pair is
cacheable iff `ExprId`s are content-addressed. The Phase 1 pool-local indices are not
suitable for cross-process caching.

The migration path is the `LocalExprId(u32) → ContentExprId(u64)` alias change documented
in expression DAG §5.4. Once `ExprId` is content-addressed, cache entries become pairs of
canonical hashes that a Redis or KV store can serve to any worker. The simplifier itself
does not change — its API operates on `ExprId` regardless of representation.

### 5.4 Float-aware simplification

Phase 1 deliberately treats `Float` as a barrier — float literals are folded together and
otherwise left alone. This avoids floating-point rewriting hazards (associativity is not
preserved: `(a + b) + c ≠ a + (b + c)` in IEEE-754) and keeps the simplifier's symbolic
guarantees crisp.

Phase 3+ may want a separate `simplify_numeric` mode for numerical workloads (e.g.,
post-`evaluate_numeric` cleanup, plot generation). It would not share an entry point with
the symbolic simplifier; it would be a sibling module.

### 5.5 Heuristic ordering and "nice form"

SCOPE.md §2.6's "no heuristic simplification ordering (no Risch-style normalization)"
limit applies to Phase 2 as well. The Phase 1 simplifier is purely syntactic — it has no
notion of "this form is prettier than that form" beyond the structural rules above. Users
who need particular normal forms (factored vs. expanded, sum-of-products, partial
fractions) call the dedicated routines (`expand`, `factor` in Phase 2, etc.).

---

## 6. Testing Strategy

### 6.1 Unit tests (`cargo test`)

**Numeric folding:**
- `simplify(2 + 3) == 5`
- `simplify(1/2 + 1/3) == 5/6` (exact rational, lowest terms)
- `simplify(2 * 3 * 5) == 30`
- `simplify((2)^10) == 1024`
- `simplify(2.0 + 3) == 5.0` (float taint)

**Like-term collection:**
- `simplify(x + x) == 2*x`
- `simplify(2*x + 3*x) == 5*x`
- `simplify(x + 2*x + 3*x) == 6*x`
- `simplify(x*y + y*x) == 2*x*y` (sort canonicalization makes `x*y` and `y*x` the same monomial)
- `simplify(x - x) == 0`
- `simplify(x + sin(y) + 2*x) == 3*x + sin(y)`

**Power consolidation:**
- `simplify(x * x) == x^2`
- `simplify(x^2 * x^3) == x^5`
- `simplify((x^2)^3) == x^6`
- `simplify((x^(1/2))^2) == x` (integer product of rational exponents)
- `simplify((x^a)^b)` left as `(x^a)^b` when `a, b` are symbolic (cannot consolidate safely)

**Rational expressions:**
- `simplify(x^2 / x) == x`
- `simplify((x^2 + x) / x) == x + 1`
- `simplify((x^2 - 1) / (x - 1)) == x + 1`
- `simplify(1 / 0)` → `KernelError::DivisionByZero`
- `simplify(x / (y - y))` → `KernelError::DivisionByZero` (after inner simplification reveals zero)

**Pythagorean:**
- `simplify(sin(x)^2 + cos(x)^2) == 1`
- `simplify(a + sin(x)^2 + b + cos(x)^2) == a + b + 1`
- `simplify(sin(2*y)^2 + cos(2*y)^2) == 1`
- `simplify(sin(x)^2 + cos(y)^2)` left unchanged (different arguments)
- `simplify(sin(x)^3 + cos(x)^2)` left unchanged (wrong exponent on sin)

**Idempotence regression:**
- For each test above, `simplify(simplify(input)) == simplify(input)`.

### 6.2 Property-based tests (`proptest`)

- **Idempotence** (SCOPE.md §1.12): for randomly generated `Expr`, `simplify(simplify(e)) == simplify(e)` structurally.
- **Numerical agreement.** Generate a symbolic expression `e` and a binding for its free
  variables (rational values). Compute `evaluate_numeric(e, binding)` and
  `evaluate_numeric(simplify(e), binding)`. Assert they agree to a tight tolerance
  (defaults: rationals exact; floats within `1e-10` relative). Catches algebraic bugs
  that pass structural tests.
- **No ExprId growth on already-simplified input.** For random `e`, the arena size before
  and after `simplify(simplify(e))` differs by at most a small constant (cache shouldn't
  re-intern).
- **Termination.** `simplify` always returns within the `MAX_ITERS` cap. Asserted via a
  pass counter in debug builds.
- **Pythagorean specifically.** Generate an expression of the form
  `Sum + sin(u)^2 + cos(u)^2` where `Sum` is arbitrary; assert the output equals
  `simplify(Sum) + 1`.

### 6.3 Benchmarks (`criterion`)

| Benchmark | Target |
|-----------|--------|
| `simplify` on a 50-term sum | <100 ms (SCOPE.md §1 success criterion) |
| `simplify` on `(x+1)^10` already expanded (~1k internal nodes via DAG sharing) | <50 ms |
| `simplify` on `x^2 / x` | <100 µs (polynomial fast path) |
| `simplify` of an already-canonical 1k-node expression | <5 ms (cache + no-op path) |
| Idempotent re-simplify of a 1k-node result | <2 ms (cache hit on every node) |

The "already-canonical" benchmark is the regression guard for caching — a no-op simplify
should be near-free. If it regresses, the cache or the bottom-up dedup is broken.

### 6.4 Fuzz testing (`cargo-fuzz`)

- **Target:** `simplify(parse(arbitrary_bytes))`. Asserts (a) no panics, (b) idempotence,
  (c) `MAX_ITERS` cap is never hit (would indicate a non-terminating rule), (d) the output
  pool's `len()` is bounded by some reasonable multiple of the input.
- **Seed corpus:** the legacy `.tst` files (curated subset that parses cleanly under
  the Phase 1 grammar) plus a small hand-curated set of pathological inputs (deeply
  nested Add/Mul, repeated Pythagorean patterns, very large coefficients).
- **Run duration:** ≥1 hour per release (combined with the parser fuzz target).

### 6.5 Golden-corpus tests (`pytest`)

A subset of `legacy/reduce-algebra-code-r7357-trunk/packages/*/{*.tst,*.rlg}` that
exercises Phase 1 simplification (per SCOPE.md §0.7 layer (d)). For each `.tst` input,
parse, simplify, and compare against the corresponding `.rlg` output. Mismatches are
investigated case-by-case — REDUCE may have made different choices about canonical form
that we elect to match exactly, deviate from, or document as an intentional difference.

The curated set lives in `tests/golden/simplify/` with a manifest mapping input file to
expected output and a `# reason: ...` annotation when our output deviates.

---

## 7. Action Items

### Phase 1 — Core implementation

1. [ ] Create `crates/monomix-kernel/src/simplify/mod.rs` exposing `simplify` and
       `simplify_with_cache`; wire into `KernelError`
2. [ ] Implement `driver.rs` — bottom-up traversal via `map_bottom_up`, fixed-point loop
       with `MAX_ITERS = 3`, debug assertion on convergence (§3.1)
3. [ ] Implement `numeric.rs` — `BigInt`/`Rational` exact arithmetic with `i64` fast path,
       float-taint behaviour (§3.2)
4. [ ] Implement `like_terms.rs` — `(coefficient, monomial)` decomposition, `FxHashMap`
       bucketing, re-emission via pool constructors (§3.3)
5. [ ] Implement `powers.rs` — `(x^a)^b → x^(a*b)` with the integer/rational guard table
       (§3.4)
6. [ ] Implement `rational.rs` — orchestration over the §1.5 polynomial division engine;
       no-cancellation fallback when remainder is non-zero (§3.5)
7. [ ] Implement `patterns.rs` — `Pattern`, `MetaVar`, `MatchEnv`, `Rule`, `RuleKind`,
       `RuleRegistry` (§3.6)
8. [ ] Implement `rules.rs` — Pythagorean rule as the sole built-in; pre-scan via
       like-term bucket function-tag bitset (§3.6)
9. [ ] Wire `RuleRegistry` ownership into the Python `Session` (per SCOPE.md §1.10)
10. [ ] Implement PyO3 boundary — GIL release for >500-node inputs, write-lock the pool
        (§3.8)

### Phase 1 — Verification

11. [ ] Unit-test all transformations enumerated in §6.1
12. [ ] `proptest` idempotence + numerical-agreement suite (§6.2)
13. [ ] `criterion` benchmarks including the "already-canonical" no-op regression guard
        (§6.3)
14. [ ] `cargo-fuzz` target with idempotence + termination + arena-bound assertions (§6.4)
15. [ ] Curate the golden-corpus `.tst`/`.rlg` subset for simplification (§6.5)
16. [ ] Confirm SCOPE.md §1.12 invariants hold: `simplify` idempotence, `expand` ∘
        `simplify` round-trip, `df` linearity (the last cross-checked once `df` lands)

### Phase 2 — Generalization (deferred)

17. [ ] Generalize `Pattern` and `MatchEnv` to support user-defined rules from REDUCE
        `let`-syntax and Python plugin builders (SCOPE.md §2.6)
18. [ ] Add confluence analysis or a deterministic application order for the rule
        database (§5.1)
19. [ ] Implement read-phase / write-phase split for parallel within-request
        simplification (§5.2; shared with expression DAG §5.2)
20. [ ] Migrate to content-addressed `ExprId` and add an MCP-side simplification cache
        (§5.3, expression DAG §5.4)
21. [ ] Add `simplify_advanced` entry point with conditional rules and trig-identity
        suite beyond Pythagorean (SCOPE.md §2.6)
