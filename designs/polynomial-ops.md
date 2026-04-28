# Polynomial Ops — System Design

**Component:** `monomix-kernel::poly`
**Status:** Design phase
**Date:** 2026-04-28
**References:** SCOPE.md §1.5, §1.7, §1.4, §0.7; ADR-0001; ADR-0002; `designs/expression-dag.md`; `designs/simplifier.md`

---

## 1. Requirements

### 1.1 Functional requirements

The polynomial-ops module is the kernel's univariate polynomial engine. It is consumed by
the simplifier (rational-expression cancellation, §3.5 of `designs/simplifier.md`) and by
direct user calls through `expand`, `collect`, `deg`, and `coeff`. It also underpins the
quadratic and linear-system solvers (SCOPE.md §1.6) that need coefficient extraction and
polynomial-form recognition.

It must support every operation listed in SCOPE.md §1.5:

- **Sparse univariate representation.** A polynomial is a sequence of `(exponent, coefficient)`
  terms in a single variable, sorted by descending exponent, with no zero coefficients and
  no duplicate exponents. Coefficients are kernel expressions (`ExprId`), not just numbers
  — `(2*y + 1) * x^3 + 5 * x` is a univariate polynomial in `x` with two non-numeric
  coefficients. This generality is what lets the simplifier feed the engine partially-
  symbolic inputs without first canonicalizing everything to ground rationals.
- **Arithmetic.** Addition, subtraction, multiplication, exponentiation by non-negative
  integer, and division-with-remainder (Euclidean): given `f, g` with `g ≠ 0`, produce
  `(q, r)` such that `f = q*g + r` and `deg(r) < deg(g)`.
- **Surface operations.** `expand(e)` distributes products and powers through sums to
  produce a flat polynomial form; `collect(e, var)` re-encodes an expression as a sum of
  terms grouped by powers of `var`; `deg(e, var)` and `coeff(e, var, n)` extract the
  highest exponent and the coefficient of `var^n` respectively.
- **View conversion.** Round-trip between `ExprId` and the internal `UnivPoly`
  representation: `view(pool, e, var) -> Result<UnivPoly>` recognizes a univariate
  polynomial in `var` (or returns a structured error indicating which subterm is not
  polynomial); `to_expr(pool, &poly) -> ExprId` re-emits a polynomial as an `ExprId`
  through the pool's normalizing constructors.
- **Polynomial-form predicates.** `is_polynomial_in(pool, e, var) -> bool`,
  `common_univariate(pool, e1, e2) -> Option<Symbol>` — used by the simplifier to decide
  whether to invoke this engine at all (§3.5 of `designs/simplifier.md`).

### 1.2 Non-functional requirements

| Requirement | Target | Rationale |
|-------------|--------|-----------|
| `add`/`sub` on n-term polynomials | O(n) | Linear merge; no GCD on the path |
| `mul` on n-term × m-term | O(n·m) | Sparse convolution; no FFT in Phase 1 |
| `divide` on degree-n / degree-m | O((n−m)·m) per quotient term | Schoolbook long division |
| `expand((x+1)^k)` for k≤20 | <100 ms | SCOPE.md §1.12 round-trip property test |
| `df` on a 20-term univariate polynomial | <50 ms | SCOPE.md §1, Phase 1 success criterion |
| Termination on any input | Always — no infinite loops | Correctness; verified by fuzz |
| Determinism | Same input ⇒ same output `ExprId` across runs | Tests + cache reproducibility |
| No `unsafe` | Required | Kernel rule (ADR-0002) |
| `Send + Sync` | Required | MCP server concurrency (ADR-0002, §0.5) |
| GIL release | Yes, for inputs >500 nodes (delegate to caller's threshold) | SCOPE.md §0.5 |

### 1.3 Constraints

- The polynomial engine reads expressions via `&ExprPool` and creates new ones via
  `&mut ExprPool::*` constructors. **It never constructs `ExprNode` directly.**
- The engine is **stateless across calls** — all working state is in `UnivPoly` values
  owned by the caller or in stack-local scratch. No global polynomial cache; the
  simplifier's `SimplifyCache` (`designs/simplifier.md` §2.1) is the only kernel-level
  cache, and it caches simplified `ExprId`s, not `UnivPoly`s.
- The engine is **bounded in Phase 1**: only the operations enumerated in §1.1 are
  supported. Out of scope: multivariate (Phase 2 §2.5), factorization (Phase 2 §2.2),
  square-free decomposition (Phase 2), Berlekamp / Cantor-Zassenhaus (Phase 2),
  modular arithmetic (Phase 3+), Groebner bases (Phase 3+).
- **Polynomial GCD is opt-in** at the simplifier's `cfg.gcd` switch
  (`designs/simplifier.md` §3.5). Phase 1 implements `gcdf`-style content/primitive-part
  GCD only when the simplifier has opted in; the polynomial engine's *core* arithmetic
  path does not invoke GCD on every operation.
- The engine **must not mutate `ExprId`s** — interned nodes are immutable. `UnivPoly`
  values are owned and may be mutated freely; they are not interned.
- **Coefficient field is exact.** Coefficients stay in the integer/rational subring of
  `ExprId`s for the fast path. Symbolic coefficients (e.g. `(2*y + 1)` in §1.1's example)
  are stored as opaque `ExprId`s and operated on through pool constructors — the engine
  does not assume coefficients commute with anything beyond addition and multiplication
  on the pool.

### 1.4 Switch defaults — implicit settings vs. REDUCE

REDUCE parameterizes its polynomial path through a handful of switches. Phase 1 ships a
fixed combination; the table below pins the implicit defaults so that golden-corpus
divergence (§6.5) is understood up front rather than rediscovered in review:

| REDUCE switch | REDUCE default | Phase 1 behaviour | Notes |
|---------------|----------------|--------------------|-------|
| `gcd`         | off | **off** (only exact-remainder cancellation) | Matches REDUCE; GCD on every op is too costly for the default path (`polrep.red:54-67`) |
| `lcm`         | off | **off** (denominator merging uses ordinary product, not LCM) | Phase 1 has no `Add` of `Div` to merge anyway |
| `expandexpt`  | on  | **on for `expand()` only**; **off for `simplify()`** | `expand()` is explicit; `simplify()` preserves `(a+b)^2` (`designs/simplifier.md` §1.4) |
| `factor`      | off | **off** (deferred to Phase 2 §2.2) | Factorization is not in Phase 1 |
| `mcd`         | on  | **off** | Matches the simplifier's choice; the polynomial engine itself doesn't force common denominators |
| `rationalize` | off | **off** (Phase 1 does not implement) | Deferred to Phase 2 |

The switches that do affect Phase 1 behaviour (`gcd`, `expandexpt`-via-explicit-`expand`)
are surfaced through the existing `SimplifierConfig` struct on `Session`; the polynomial
engine itself does not own a config — it takes its instructions per-call from the
caller. See §2.1.

---

## 2. High-Level Design

### 2.1 Public API

```rust
/// A sparse univariate polynomial in `variable`. Terms are sorted by descending exponent
/// with no zero coefficients and no duplicate exponents. The zero polynomial is the
/// empty term list; the variable is still recorded so two zero polynomials in different
/// variables remain distinguishable for diagnostics.
#[derive(Clone, Debug)]
pub struct UnivPoly {
    pub variable: Symbol,                         // ExprId of the indeterminate
    pub terms: SmallVec<[Term; 8]>,               // descending by exponent
}

#[derive(Clone, Debug)]
pub struct Term {
    pub exponent: u32,                            // non-negative; symbolic exponents go through ExprId path (§3.7)
    pub coefficient: ExprId,                      // any ExprId not containing `variable`
}

/// View an expression as a univariate polynomial in `var`. Returns `Err(NotPolynomial)`
/// if `e` cannot be expressed in this form (e.g. `sin(var)`, `var^var`, `1/(var+1)` —
/// but `1/2 * var^3` is fine because `1/2` is a coefficient).
pub fn view(pool: &ExprPool, e: ExprId, var: Symbol) -> Result<UnivPoly, PolyError>;

/// Re-emit a UnivPoly as an ExprId, normalized through pool constructors.
/// Empty term list → `pool.zero`. Single term with exponent 0 → bare coefficient.
pub fn to_expr(pool: &mut ExprPool, p: &UnivPoly) -> ExprId;

/// Polynomial arithmetic. Each operation takes &UnivPoly inputs and returns a new
/// UnivPoly; coefficient ops route through &mut ExprPool because they may construct
/// new coefficient expressions.
pub fn add(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> Result<UnivPoly, PolyError>;
pub fn sub(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> Result<UnivPoly, PolyError>;
pub fn mul(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> Result<UnivPoly, PolyError>;
pub fn pow(pool: &mut ExprPool, a: &UnivPoly, k: u32)         -> Result<UnivPoly, PolyError>;
pub fn neg(pool: &mut ExprPool, a: &UnivPoly)                  -> UnivPoly;

/// Euclidean division with remainder: a = q*b + r, deg(r) < deg(b). Returns
/// `Err(DivisionByZero)` if b is the zero polynomial.
pub fn divide(
    pool: &mut ExprPool,
    a: &UnivPoly,
    b: &UnivPoly,
) -> Result<(UnivPoly, UnivPoly), PolyError>;

/// Surface operations that operate on ExprId rather than UnivPoly. Internally these
/// call view + arithmetic + to_expr. They are the entry points the parser binds and
/// that the Python `Session` exposes via PyO3.
pub fn expand(pool: &mut ExprPool, e: ExprId) -> Result<ExprId, PolyError>;
pub fn collect(pool: &mut ExprPool, e: ExprId, var: Symbol) -> Result<ExprId, PolyError>;
pub fn deg(pool: &ExprPool, e: ExprId, var: Symbol) -> Result<u32, PolyError>;
pub fn coeff(pool: &mut ExprPool, e: ExprId, var: Symbol, n: u32) -> Result<ExprId, PolyError>;

/// Predicates used by the simplifier to decide engine applicability without
/// constructing a full UnivPoly.
pub fn is_polynomial_in(pool: &ExprPool, e: ExprId, var: Symbol) -> bool;
pub fn common_univariate(pool: &ExprPool, a: ExprId, b: ExprId) -> Option<Symbol>;

#[derive(Clone, Debug, thiserror::Error)]
pub enum PolyError {
    #[error("expression is not polynomial in {var}: subterm {kind}")]
    NotPolynomial { var: Symbol, kind: NotPolyKind, span: Option<Span> },
    #[error("division by the zero polynomial")]
    DivisionByZero { span: Option<Span> },
    #[error("exponent {0} exceeds Phase 1 limit (u32::MAX)")]
    ExponentOverflow(u64),
    #[error("multivariate input not supported in Phase 1")]
    Multivariate { other_var: Symbol, span: Option<Span> },
}

#[derive(Clone, Debug)]
pub enum NotPolyKind {
    /// var appears inside a function (e.g. `sin(var)`).
    NonPolynomialFunction(FnTag),
    /// Negative or symbolic exponent on `var`.
    NonNaturalExponent { exponent: ExprId },
    /// var appears in the denominator of a Div.
    InDenominator,
    /// var appears as both base and exponent in a Pow (e.g. `var^var`).
    SelfReferential,
}
```

`PolyError::NotPolynomial` is structured so the simplifier (`designs/simplifier.md` §3.5)
can pattern-match on `NotPolyKind` rather than parsing strings: when `view()` fails with
`NonPolynomialFunction` it gives up gracefully and leaves the original `Div` intact, but
when it fails with `Multivariate` the Phase 2 multivariate engine becomes the natural
revisit. This explicit categorization is also what the parser's `SpanMap` attaches to,
so user-facing diagnostics can highlight the exact subterm.

### 2.2 Component diagram

```
   ExprId (input)            UnivPoly (input)
         │                          │
         ▼                          ▼
   ┌──────────┐               ┌──────────┐
   │  view.rs │               │ ops.rs   │  add/sub/mul/pow/neg/divide
   │ (recognize│               │ (term-list│   sparse merge / convolution
   │  + extract│               │  algebra) │   schoolbook division
   │  terms)  │               └─────┬────┘
   └────┬─────┘                     │
        │ UnivPoly                  │ UnivPoly
        ▼                           ▼
   ┌──────────────────────────────────────┐
   │              emit.rs                 │  to_expr: re-encode as ExprId
   │  (sort, drop zeros, route through    │  through pool constructors
   │   pool.add / pool.mul / pool.pow)    │
   └──────────────┬───────────────────────┘
                  │ ExprId (output)
                  ▼
   ┌──────────────────────────────────────┐
   │           surface.rs                 │  expand / collect / deg / coeff
   │  (user-facing entry points; thin     │
   │   wrappers over view + ops + emit)   │
   └──────────────────────────────────────┘
```

### 2.3 Module layout

```
crates/monomix-kernel/src/poly/
├── mod.rs           — public API, PolyError, re-exports
├── repr.rs          — UnivPoly, Term, invariants & debug_assert helpers
├── view.rs          — ExprId → UnivPoly (recognizer + extractor)
├── emit.rs          — UnivPoly → ExprId (re-emission through pool)
├── ops.rs           — add, sub, mul, pow, neg, divide (term-list algebra)
├── surface.rs       — expand, collect, deg, coeff, is_polynomial_in, common_univariate
├── gcd.rs           — content/primitive-part GCD (used only when simplifier opts in)
└── tests.rs
```

The split mirrors the kernel convention from `simplify/`: arithmetic engines, view
converters, and surface entry points are kept in separate modules so each is testable in
isolation. `gcd.rs` is its own module rather than a function in `ops.rs` because Phase 2
factorization (§2.2 of SCOPE.md) will live here too — the file boundary is sized for the
larger eventual surface.

### 2.4 Algorithm choices at a glance

| Operation | Algorithm | Complexity | Notes |
|-----------|-----------|------------|-------|
| `add`, `sub` | Two-pointer merge of sorted term lists | O(n+m) | No allocation when one side is shorter than the other's tail |
| `mul` | Sparse convolution + heap-merge of partial sums | O(n·m·log(min(n,m))) | Heap is `BinaryHeap<(exp, idx_a, idx_b)>`; see §3.3 |
| `pow` | Repeated squaring with `mul` | O(log k) multiplications | Square-and-multiply on the polynomial |
| `divide` | Schoolbook long division | O((n−m)·m) coefficient ops | No fast-division algorithm in Phase 1 |
| `expand` | View-rewrite-emit, with `mul`/`pow` driving the distribution | Bounded by output size | Polynomial expansion is "make explicit what was structural" |
| `collect` | View as polynomial → `to_expr` | O(n) terms in result | Reuses view + emit, no separate algorithm |
| `view` | Single recursive walk; classifies each Mul factor as coefficient / power-of-var / failure | O(size of input ExprNode tree) | Memoized on `ExprId` per-call |
| `deg`, `coeff` | View, then index into terms | O(view) + O(n) | `deg` is just `terms[0].exponent` after view |

The conspicuous absence is FFT-based multiplication. Phase 1's success criterion (50-term
sums in <100 ms) does not require it; the §6.3 benchmarks pin the regression line, and
the complexity migration path is a single algorithm swap inside `ops::mul` (see §5.1).

### 2.5 Single-variable focus, not single-variable forever

Phase 1 is univariate. Multivariate (SCOPE.md §2.5) is a Phase 2 deliverable. The key
design choice that makes the Phase 2 step additive rather than rewriting:

- `UnivPoly { variable, terms }` is the Phase 1 type. Its eventual cousin is
  `MultiPoly { vars: Box<[Symbol]>, terms: SmallVec<[(Vec<u32>, ExprId); 8]> }` with an
  explicit monomial-order parameter.
- `view`'s recursive walk in Phase 1 fails with `Multivariate { other_var, .. }` when it
  encounters another symbol; Phase 2 changes the failure into a recursive view on the
  multivariate type. The arithmetic in `ops.rs` is replaced wholesale (Buchberger-like
  reductions for division, S-polynomial for GCD) but the surface API (`expand`,
  `collect`, `deg`, `coeff`) gains an extra parameter and the public functions delegate.
- The simplifier's call sites (`designs/simplifier.md` §3.5) already handle
  `view → Err(Multivariate) → return original Div` — when Phase 2 lands, those sites
  pivot from "give up" to "view as multivariate" without changing the surrounding
  control flow.

This deliberate forward-compat shape is documented up front so the Phase 2 work in §5.1
is a renaming exercise plus a new arithmetic kernel, not an interface restructuring.

---

## 3. Deep Dive

### 3.1 Sparse representation invariants (`repr.rs`)

```rust
pub struct UnivPoly {
    pub variable: Symbol,
    pub terms: SmallVec<[Term; 8]>,
}
```

Invariants (asserted in debug builds via `debug_assert!`, never panicked in release):

1. **Sorted descending by exponent.** `terms[i].exponent > terms[i+1].exponent` for all
   `i`. This means `terms[0]` is the leading term in O(1), and merge-style add/sub is
   linear.
2. **No zero coefficients.** A term whose coefficient simplifies to `pool.zero` is
   dropped at construction time. This keeps `add`/`sub`/`mul` outputs canonical without
   a separate cleanup pass.
3. **No duplicate exponents.** Construction (in `view`, `add`, `mul`) collapses any
   collision by adding the coefficients through the pool.
4. **Coefficient does not contain `variable`.** Verified by `view`; an `ExprId` that
   contains `variable` cannot be a coefficient. This is the precondition that makes
   `add`/`sub`/`mul` term-mergeable at all.
5. **Empty term list ⟺ zero polynomial.** `is_zero(p)` is `p.terms.is_empty()`. The
   variable is still recorded; this matters for diagnostics when two operands disagree
   (`add` of two zero polynomials in different variables raises `Multivariate`).

The inline `SmallVec` buffer is sized at 8 because human-written polynomials and
post-`expand` outputs of degree ≤7 fit without heap allocation. The §6.3 benchmark
"`expand((x+1)^10)`" allocates because 11 terms exceeds the inline cap; this is
deliberate — the inline cap is sized for the common case, not the headline benchmark.
The 32-element inline cap used in the simplifier (`designs/simplifier.md` §3.2) is
chosen because that buffer holds *all children* of a single Add/Mul; here a single
polynomial typically has fewer terms than a single Add has children.

`Term::exponent` is `u32`. Reasoning: REDUCE caps polynomial exponents at machine
integers in practice (the legacy code uses `eqn(cdr u, cdr v)` on raw fixnums); a
`u32` gives ~4 billion as the cap, which is well past any user-meaningful polynomial
degree. `pow(_, k)` rejects `k > u32::MAX` with `ExponentOverflow`. The simplifier's
`(x^a)^b → x^(a*b)` rule (`designs/simplifier.md` §3.4) checks for `u32` overflow
before invoking `mul` on exponents.

### 3.2 View — recognizing a polynomial (`view.rs`)

`view(pool, e, var) -> Result<UnivPoly, PolyError>` is the recognizer. It walks the
expression DAG once, partitioning each subterm into "polynomial in `var`" vs "fail".

```rust
pub fn view(pool: &ExprPool, e: ExprId, var: Symbol) -> Result<UnivPoly, PolyError> {
    let mut buckets: FxHashMap<u32, Vec<ExprId>> = FxHashMap::default();
    view_into(pool, e, var, Sign::Pos, &mut buckets)?;
    finalize(pool, var, buckets)
}

fn view_into(
    pool: &ExprPool,
    e: ExprId,
    var: Symbol,
    sign: Sign,
    buckets: &mut FxHashMap<u32, Vec<ExprId>>,
) -> Result<(), PolyError> {
    match pool.get(e) {
        // Sum: distribute view over children.
        ExprNode::Add(children) => {
            for &c in children {
                view_into(pool, c, var, sign, buckets)?;
            }
            Ok(())
        }

        // Subtraction is normalized away by the pool (Neg(x) interns as -1*x or as
        // its own variant), so we hit Neg here rather than a Sub variant.
        ExprNode::Neg(inner) => view_into(pool, *inner, var, sign.flip(), buckets),

        // Product: extract the var-power factor; everything else is the coefficient.
        ExprNode::Mul(children) => view_product(pool, children, var, sign, buckets),

        // Bare power: x^k is (k, 1), x^non-natural is an error.
        ExprNode::Pow(base, exp) if *base == var => {
            let k = extract_natural_exponent(pool, *exp, var)?;
            buckets.entry(k).or_default().push(sign.apply(pool, pool.one));
            Ok(())
        }

        // Bare variable.
        ExprNode::Symbol(s) if Symbol(*s) == var => {
            buckets.entry(1).or_default().push(sign.apply(pool, pool.one));
            Ok(())
        }

        // Anything else: must be a constant w.r.t. var.
        _ => {
            if contains_symbol(pool, e, var) {
                Err(reject_kind(pool, e, var))
            } else {
                buckets.entry(0).or_default().push(sign.apply(pool, e));
                Ok(())
            }
        }
    }
}
```

`view_product` is the substantive case: it scans `Mul` children once, picking out at
most one factor that contributes a `var^k` (the others must all be coefficients). If
two children both reference `var` (e.g. `var * var^2`) the simplifier will already have
collapsed them through the pool's eager normalizations — but as a defensive fallback
this case is detected and the exponents summed before bucketing. If one child contains
`var` non-polynomially (e.g. `sin(var)`) the function returns the matching
`NotPolynomial` error.

```rust
fn view_product(
    pool: &ExprPool,
    children: &[ExprId],
    var: Symbol,
    sign: Sign,
    buckets: &mut FxHashMap<u32, Vec<ExprId>>,
) -> Result<(), PolyError> {
    let mut exponent: u32 = 0;
    let mut coeff_factors: SmallVec<[ExprId; 8]> = SmallVec::new();

    for &c in children {
        match classify(pool, c, var)? {
            Factor::Constant(id)         => coeff_factors.push(id),
            Factor::VarPower(k)          => exponent = exponent.checked_add(k)
                                                .ok_or(PolyError::ExponentOverflow(k as u64))?,
        }
    }

    let coeff = match coeff_factors.as_slice() {
        []          => pool.one,
        [single]    => *single,
        many        => pool.mul(many.to_vec()),
    };
    buckets.entry(exponent).or_default().push(sign.apply(pool, coeff));
    Ok(())
}
```

The bucket map is `FxHashMap<u32, Vec<ExprId>>`: the key is the exponent, the value is
the list of coefficient `ExprId`s contributing to that power. `finalize` sums each
bucket through `pool.add(...)` (which simplifies via the pool's eager normalizations
plus the coefficient simplifier, when one is in scope), drops zero coefficients, and
returns terms sorted descending. For Phase 1's polynomial sizes (≤ a few hundred
terms in the benchmark cases), an `FxHashMap` is the right pick over a sort-and-fold.

**Why a HashMap, not a sort-by-exponent.** Exponents repeat in input — `x + x^2 + 3*x +
x^2` has only two distinct exponents. A sort-then-coalesce pass is O(n log n) on the
expanded child count; the hashmap is O(n) and matches the simplifier's like-term
strategy (`designs/simplifier.md` §3.3) so both layers use the same primitive.

**Defensive overflow.** `exponent.checked_add` guards against attacker-controlled
inputs. `pow(big, big)` after the simplifier folds rationals can technically push past
`u32::MAX`. The `view` path raises `ExponentOverflow` rather than truncating; the
simplifier (`designs/simplifier.md` §3.4) won't construct such expressions through its
guarded rule, but `view` is reachable from `expand`/`collect` directly.

### 3.3 Arithmetic (`ops.rs`)

**Add / sub.** Two-pointer merge of sorted term lists. Equal exponents collapse by
adding the coefficients through `pool.add`; if the resulting coefficient is `pool.zero`
the term is dropped.

```rust
pub fn add(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> Result<UnivPoly, PolyError> {
    if a.variable != b.variable {
        if a.terms.is_empty() { return Ok(b.clone()); }
        if b.terms.is_empty() { return Ok(a.clone()); }
        return Err(PolyError::Multivariate { other_var: b.variable, span: None });
    }
    let mut out: SmallVec<[Term; 8]> = SmallVec::with_capacity(a.terms.len() + b.terms.len());
    let (mut i, mut j) = (0, 0);
    while i < a.terms.len() && j < b.terms.len() {
        match a.terms[i].exponent.cmp(&b.terms[j].exponent) {
            Ordering::Greater => { out.push(a.terms[i].clone()); i += 1; }
            Ordering::Less    => { out.push(b.terms[j].clone()); j += 1; }
            Ordering::Equal   => {
                let coeff = pool.add(vec![a.terms[i].coefficient, b.terms[j].coefficient]);
                if coeff != pool.zero {
                    out.push(Term { exponent: a.terms[i].exponent, coefficient: coeff });
                }
                i += 1; j += 1;
            }
        }
    }
    out.extend_from_slice(&a.terms[i..]);
    out.extend_from_slice(&b.terms[j..]);
    Ok(UnivPoly { variable: a.variable, terms: out })
}
```

`sub(pool, a, b)` is `add(pool, a, &neg(pool, b))`. `neg` walks the term list and routes
each coefficient through `pool.neg`; the term order is preserved.

**Multiplication.** Sparse convolution with a heap-driven merge.

```rust
pub fn mul(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> Result<UnivPoly, PolyError> {
    if a.terms.is_empty() || b.terms.is_empty() {
        return Ok(UnivPoly { variable: a.variable, terms: SmallVec::new() });
    }
    if a.variable != b.variable {
        return Err(PolyError::Multivariate { other_var: b.variable, span: None });
    }

    // Heap holds (exponent, i, j) for each pair we have not yet yielded. We start by
    // pushing all (a[i], b[0]) pairs; each pop advances j for that i. This is the
    // sparse-convolution trick from Monagan & Pearce (2007) — it gives O(n·m) work
    // with O(n) auxiliary memory rather than O(n·m) as a dense matrix would.
    let mut heap: BinaryHeap<HeapEntry> = BinaryHeap::with_capacity(a.terms.len());
    let mut next_j: Vec<usize> = vec![0; a.terms.len()];
    for i in 0..a.terms.len() {
        heap.push(HeapEntry { exponent: a.terms[i].exponent + b.terms[0].exponent, i, j: 0 });
    }

    let mut out: SmallVec<[Term; 8]> = SmallVec::new();
    while let Some(HeapEntry { exponent, i, j }) = heap.pop() {
        let coeff = pool.mul(vec![a.terms[i].coefficient, b.terms[j].coefficient]);
        // Merge with the in-flight top term if the exponent matches.
        if let Some(last) = out.last_mut().filter(|t| t.exponent == exponent) {
            last.coefficient = pool.add(vec![last.coefficient, coeff]);
            if last.coefficient == pool.zero { out.pop(); }
        } else if coeff != pool.zero {
            out.push(Term { exponent, coefficient: coeff });
        }
        // Push the next pair from this i-row.
        let nj = j + 1;
        if nj < b.terms.len() {
            next_j[i] = nj;
            heap.push(HeapEntry { exponent: a.terms[i].exponent + b.terms[nj].exponent, i, j: nj });
        }
    }
    Ok(UnivPoly { variable: a.variable, terms: out })
}
```

The heap discipline is the same one used by Maple, Singular, and most modern sparse
polynomial libraries. Crucial properties: (a) the output emerges in descending-exponent
order, so no post-sort is required; (b) memory is O(n) rather than O(n·m); (c) the
hot inner loop is a single pool-arithmetic call and a heap push/pop, both tight.

**Why not a dense convolution.** Dense storage of degree-`d` polynomials uses `d+1`
slots regardless of sparsity. For `(x^100 + 1) * (x^100 + 1)` (3 terms output) the
dense path allocates 201 slots and performs 201·201 = 40k coefficient zero-checks; the
sparse path performs 3 multiplications. Phase 1's success criterion (50-term sums in
<100 ms) is dominated by the sparsity ratio of inputs the simplifier produces, which is
typically low.

**Why not FFT.** FFT-based multiplication wins for `n*m ≫ 10^4`. Phase 1's targets put
us at most in the n*m ≈ 10^3 regime, where the FFT setup cost (NTT modulus selection,
bit-reversal, normalization) loses to the constant-factor advantage of the heap merge.
Phase 3+'s revisit (§5.1) is the place to switch.

**Exponentiation.** Repeated-squaring on `mul`:

```rust
pub fn pow(pool: &mut ExprPool, a: &UnivPoly, mut k: u32) -> Result<UnivPoly, PolyError> {
    if k == 0 {
        return Ok(UnivPoly {
            variable: a.variable,
            terms: smallvec![Term { exponent: 0, coefficient: pool.one }],
        });
    }
    let mut base = a.clone();
    let mut acc: Option<UnivPoly> = None;
    while k > 0 {
        if k & 1 == 1 {
            acc = Some(match acc {
                None => base.clone(),
                Some(prev) => mul(pool, &prev, &base)?,
            });
        }
        k >>= 1;
        if k > 0 { base = mul(pool, &base, &base)?; }
    }
    Ok(acc.unwrap())
}
```

Naive repeated multiplication (`a^k` as `a*a*…*a` k times) is `k-1` multiplications;
square-and-multiply is `O(log k)`. For `(x+1)^20`, the difference is 19 vs 5
multiplications — meaningful at the §6.3 benchmark sizes.

**Division-with-remainder.** Schoolbook long division:

```rust
pub fn divide(
    pool: &mut ExprPool,
    a: &UnivPoly,
    b: &UnivPoly,
) -> Result<(UnivPoly, UnivPoly), PolyError> {
    if b.terms.is_empty() {
        return Err(PolyError::DivisionByZero { span: None });
    }
    if a.variable != b.variable {
        return Err(PolyError::Multivariate { other_var: b.variable, span: None });
    }

    let lead_b = &b.terms[0];
    let mut rem = a.clone();
    let mut quot: SmallVec<[Term; 8]> = SmallVec::new();

    while !rem.terms.is_empty() && rem.terms[0].exponent >= lead_b.exponent {
        let lead_r = &rem.terms[0];
        // Quotient term: lead_r / lead_b. Coefficient is lead_r.coeff / lead_b.coeff,
        // routed through the pool's div constructor; if the lead_b coefficient does
        // not exactly divide lead_r's, we still produce a Div node and let the
        // simplifier handle it. The `quotient_remainder_invariant` test (§6.1) covers
        // the symbolic-coefficient case.
        let coeff_q = pool.div(lead_r.coefficient, lead_b.coefficient);
        let exp_q = lead_r.exponent - lead_b.exponent;
        let qterm = Term { exponent: exp_q, coefficient: coeff_q };

        // rem -= qterm * b  (computed inline via the merge-subtract path)
        let scaled_b = scale(pool, b, &qterm)?;
        rem = sub(pool, &rem, &scaled_b)?;

        quot.push(qterm);
    }

    Ok((UnivPoly { variable: a.variable, terms: quot },
        rem))
}
```

`scale(pool, b, &qterm)` multiplies each term of `b` by `qterm.coefficient` and adds
`qterm.exponent` to each term's exponent — a lightweight specialization of `mul` for
the multiplicand-is-monomial case.

**Termination of `divide`.** Each iteration strictly decreases `rem.terms[0].exponent`
(by removing the leading term and subtracting a polynomial whose leading exponent
matches). Since exponents are non-negative `u32`, the loop terminates in at most
`a.terms[0].exponent + 1` iterations.

**Remainder canonicality.** When `b` has a non-numeric leading coefficient (e.g.
`b = (y+1)*x + 1`), the quotient terms carry `Div` nodes that won't be exactly
divisible. The §6.1 test `quotient_remainder_invariant` (a*b + r ≡ original) holds
regardless, but the `quot` and `rem` polynomials may contain unsimplified `Div` nodes
in their coefficients. Callers (notably `simplify_div` in `designs/simplifier.md` §3.5)
are expected to invoke the simplifier on each coefficient if a fully-canonical result
is needed. The polynomial engine itself does not invoke the simplifier — that would
create an inversion of dependency and a re-entrancy hazard.

### 3.4 Emit — `to_expr` (`emit.rs`)

```rust
pub fn to_expr(pool: &mut ExprPool, p: &UnivPoly) -> ExprId {
    if p.terms.is_empty() { return pool.zero; }
    let mut sum_children: SmallVec<[ExprId; 16]> = SmallVec::new();
    for term in &p.terms {
        let var_part = match term.exponent {
            0 => None,
            1 => Some(p.variable.0),                      // bare variable
            k => Some(pool.pow(p.variable.0, pool.integer(k as i64))),
        };
        let term_expr = match (var_part, term.coefficient == pool.one) {
            (None,   _    ) => term.coefficient,            // pure constant
            (Some(v), true ) => v,                          // coefficient is 1
            (Some(v), false) => pool.mul(vec![term.coefficient, v]),
        };
        sum_children.push(term_expr);
    }
    if sum_children.len() == 1 {
        sum_children.into_iter().next().unwrap()
    } else {
        pool.add(sum_children.into_vec())
    }
}
```

Re-emission routes through `pool.mul` and `pool.add`, which sort children canonically
(expression DAG §3.1 Invariant 4) and apply the eager normalizations
(`pool.pow(_, 0) → 1`, `pool.mul([_, 1]) → _`, etc.). This means `to_expr` emits a
canonical-form expression without the polynomial engine itself owning canonicalization
logic — the responsibility lives in the pool, where it's already carefully designed.

### 3.5 Surface operations (`surface.rs`)

`expand`, `collect`, `deg`, and `coeff` are thin orchestrators over `view` and `to_expr`.

**`expand`.** Distributes products and powers through sums to produce a flat polynomial
form. The implementation is a recursive tree walk rather than a `view` call because
`expand` operates *before* knowing a variable; it must distribute regardless of which
symbol is the indeterminate.

```rust
pub fn expand(pool: &mut ExprPool, e: ExprId) -> Result<ExprId, PolyError> {
    match pool.get(e) {
        ExprNode::Add(children) => {
            let expanded: Vec<ExprId> = children.iter().map(|&c| expand(pool, c)).collect::<Result<_,_>>()?;
            Ok(pool.add(expanded))
        }
        ExprNode::Mul(children) => {
            // Expand each child, then pairwise-distribute Adds.
            let expanded: Vec<ExprId> = children.iter().map(|&c| expand(pool, c)).collect::<Result<_,_>>()?;
            distribute_product(pool, &expanded)
        }
        ExprNode::Pow(base, exp) => {
            let base_expanded = expand(pool, *base)?;
            match pool.get(*exp) {
                ExprNode::SmallInt(k) if *k >= 0 && *k <= EXPAND_POW_LIMIT as i64 => {
                    distribute_power(pool, base_expanded, *k as u32)
                }
                _ => Ok(pool.pow(base_expanded, *exp)),
            }
        }
        ExprNode::Neg(inner) => {
            let inner_expanded = expand(pool, *inner)?;
            Ok(pool.neg(inner_expanded))
        }
        _ => Ok(e),
    }
}

const EXPAND_POW_LIMIT: u32 = 100;
```

`distribute_product` takes a list of expressions, identifies which are sums, and emits
the cartesian product of (sum-children × non-sum-factors) re-summed. `distribute_power`
recursively halves the exponent (taking advantage of `(a+b)^(2k) = ((a+b)^k)^2`), then
finally calls `distribute_product` on the multiplicative form. The exponent cap
`EXPAND_POW_LIMIT = 100` is a defensive backstop: `(a+b)^100` produces 101 terms with
coefficients up to `100!/50!^2 ≈ 10^29` (well within `BigInt`), but `(a+b)^1_000_000`
generates an astronomical output. The cap is set high enough that all SCOPE.md
benchmarks pass and low enough that pathological inputs don't OOM the kernel.
Hitting the cap returns `PolyError::ExponentOverflow` with the exponent value — this
is a load-bearing contract for the fuzz harness (§6.4), which generates random
`(big)^k` shapes and would otherwise sit indefinitely.

**`collect`.** Rewrites an expression as a sum of `(coefficient_in_other_vars) * var^k`
terms. This is `view + to_expr`:

```rust
pub fn collect(pool: &mut ExprPool, e: ExprId, var: Symbol) -> Result<ExprId, PolyError> {
    let p = view(pool, e, var)?;
    Ok(to_expr(pool, &p))
}
```

The work is in `view` correctly identifying coefficients (which are themselves arbitrary
expressions in *other* variables, possibly including products and sums); the result is
canonical because `to_expr` routes through pool constructors.

**`deg(e, var)`.** O(view) + O(1):

```rust
pub fn deg(pool: &ExprPool, e: ExprId, var: Symbol) -> Result<u32, PolyError> {
    let p = view(pool, e, var)?;
    Ok(p.terms.first().map(|t| t.exponent).unwrap_or(0))
}
```

The convention `deg(0) = 0` is REDUCE-compatible (the `degr` operator in
[poly/legacy/polrep.red](../legacy/reduce-algebra-code-r7357-trunk/packages/poly/polrep.red)
returns 0 for the zero polynomial). Some authorities use `deg(0) = -∞`; we explicitly
choose REDUCE's convention so the golden corpus matches.

**`coeff(e, var, n)`.** O(view) + O(n):

```rust
pub fn coeff(pool: &mut ExprPool, e: ExprId, var: Symbol, n: u32) -> Result<ExprId, PolyError> {
    let p = view(pool, e, var)?;
    Ok(p.terms.iter()
        .find(|t| t.exponent == n)
        .map(|t| t.coefficient)
        .unwrap_or(pool.zero))
}
```

Missing exponents return `pool.zero`, again matching REDUCE's `coeff` behaviour.

**`is_polynomial_in(pool, e, var)`.** A predicate that walks the tree without building a
`UnivPoly`, used by the simplifier to decide whether to invoke the engine at all:

```rust
pub fn is_polynomial_in(pool: &ExprPool, e: ExprId, var: Symbol) -> bool {
    match pool.get(e) {
        ExprNode::Add(children) | ExprNode::Mul(children) => children.iter().all(|&c| is_polynomial_in(pool, c, var)),
        ExprNode::Pow(base, exp) => {
            // var^k where k is a natural integer; or non-var^anything.
            if base_contains_var(pool, *base, var) {
                matches!(pool.get(*exp), ExprNode::SmallInt(k) if *k >= 0)
            } else {
                !contains_symbol(pool, *exp, var)
            }
        }
        ExprNode::Neg(inner) => is_polynomial_in(pool, *inner, var),
        ExprNode::Fn(_, args) => args.iter().all(|&a| !contains_symbol(pool, a, var)),
        _ => true,
    }
}
```

This is O(n) on the tree size, with no `UnivPoly` allocation. It's the cheap "should I
even try?" gate before `view`.

**`common_univariate(pool, a, b)`.** Returns `Some(var)` if both `a` and `b` are
polynomials in the same single variable, `None` otherwise. The simplifier uses this to
decide whether `simplify_div` can delegate to the polynomial engine
(`designs/simplifier.md` §3.5). Implementation: collect the symbol set of each side
(walking the tree, ignoring symbols inside `Fn` arguments per the rule above), check
that both are singletons, and that they match.

### 3.6 GCD on the simplifier opt-in path (`gcd.rs`)

When `cfg.gcd = true` (the simplifier's opt-in switch, `designs/simplifier.md` §3.5),
`simplify_div` invokes `gcd_univariate(pool, &num, &den)` and divides both sides by the
result before re-encoding. The Phase 1 implementation is content/primitive-part GCD
specialized to univariate:

```rust
/// Greatest-common-divisor of two univariate polynomials. Used only when the simplifier's
/// `cfg.gcd` switch is on (default off, matching REDUCE's `off gcd`).
pub fn gcd_univariate(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> Result<UnivPoly, PolyError> {
    if a.terms.is_empty() { return Ok(b.clone()); }
    if b.terms.is_empty() { return Ok(a.clone()); }
    let mut x = a.clone();
    let mut y = b.clone();
    while !y.terms.is_empty() {
        let (_, r) = divide(pool, &x, &y)?;
        x = y;
        y = r;
    }
    // x is now the GCD (up to a unit). Normalize the leading coefficient — for
    // numeric leads we divide out, for symbolic leads we leave it (the caller can
    // simplify the resulting form).
    Ok(monic_if_numeric(pool, &x))
}
```

This is the Euclidean algorithm, lifted from rational-coefficient text-book proofs. Two
caveats specific to the kernel:

1. **Coefficient growth.** Naive Euclidean over `Q[x]` exhibits coefficient blow-up
   (intermediate coefficients can grow exponentially in the degree). REDUCE uses
   subresultant or content-and-primitive-part variants
   ([polgcd in poly/polrep.red:.../packages/poly/](../legacy/reduce-algebra-code-r7357-trunk/packages/poly/))
   to bound this. Phase 1 uses content-and-primitive-part: factor out the integer
   content of each polynomial (via gcd of integer coefficients), run Euclidean on the
   primitive parts, then re-multiply by the gcd of the contents. This keeps
   intermediate coefficients bounded for the rational-coefficient case. Symbolic
   coefficients (`(y+1)*x + 1`) defeat content extraction; the engine falls back to
   raw Euclidean and accepts the blow-up — the simplifier flag is opt-in for exactly
   this reason.
2. **Symbolic coefficients can hide divisions.** When `lead_b` is symbolic, the
   `pool.div(lead_a, lead_b)` in `divide` returns a `Div` node that can re-enter the
   simplifier later. Phase 1's `gcd_univariate` makes no attempt to detect this; if
   the user has enabled `cfg.gcd` on a symbolic-coefficient input, they are signing up
   for whatever shape falls out. The simplifier's surrounding wrap (§3.5 of
   `designs/simplifier.md`) re-runs `simplify` on the result, which catches the most
   obvious cases.

**Why no Bareiss / subresultant in Phase 1.** Bareiss and subresultant give exact
integer-domain GCDs without the full content-and-primitive-part dance, but they're
optimized for dense polynomials over `Z`. Our coefficients are `ExprId`s that may be
rationals or arbitrary expressions; the speedup from Bareiss disappears on the symbolic
branch and the implementation cost is ~3× the content-and-primitive-part path. Phase 2
factorization (SCOPE.md §2.2) is the right time to reach for subresultant — it's the
prerequisite for square-free factorization anyway.

### 3.7 Symbolic exponents — explicitly out of scope

Phase 1's `view` rejects any `Pow(var, e)` where `e` is not a non-negative `SmallInt`.
This means `x^a` (where `a` is a symbol) is *not* a polynomial in `x` for the
engine's purposes, and the simplifier's polynomial-cancellation path
(`designs/simplifier.md` §3.5) gives up gracefully on such expressions.

This is a deliberate scope cut. REDUCE supports symbolic exponents in some contexts
(`(x^a)^b → x^(a*b)`, certain GCD computations) but at the cost of significant complexity
and unsoundness around real-vs-complex branch cuts. The simplifier's Phase 1 power rule
(`designs/simplifier.md` §3.4) restricts `(x^a)^b` consolidation to integer/rational
exponents for the same reason. Putting the same guard at the polynomial engine entry
keeps the two layers consistent.

### 3.8 PyO3 boundary

The polynomial-ops module is exposed to Python at the `expand`, `collect`, `deg`, and
`coeff` surface operations. Each follows the same pattern as the simplifier's PyO3
boundary (`designs/simplifier.md` §3.8):

```rust
#[pyfunction]
fn expand(py: Python<'_>, session: &PySession, expr: &PyExpr) -> PyResult<PyExpr> {
    let pool_handle = expr.pool.clone();
    let id = expr.id;
    let subtree_size = pool_handle.read().subtree_size(id);
    let new_id = if subtree_size > 500 {
        py.allow_threads(|| {
            let mut pool = pool_handle.write();
            monomix_kernel::poly::expand(&mut pool, id)
        })?
    } else {
        let mut pool = pool_handle.write();
        monomix_kernel::poly::expand(&mut pool, id)?
    };
    Ok(PyExpr { pool: pool_handle, id: new_id })
}
```

GIL release uses the same `subtree_size > 500` threshold the simplifier uses. There is
no `PolyOpsConfig` analog of `SimplifierConfig` — the polynomial engine takes its
behaviour from the call site, with `expand`/`collect` being unconditional and `gcd`
being controlled by the simplifier's own config when called from there.

### 3.9 Error handling

| Error | Source | Handling |
|-------|--------|----------|
| `PolyError::DivisionByZero` | `divide` with `b.terms.is_empty()` | Return immediately with span if available |
| `PolyError::Multivariate` | `view` finds a second variable; arithmetic finds two operands with different variables | Return; simplifier and `expand`/`collect` translate to "leave unchanged" |
| `PolyError::NotPolynomial` | `view` finds `sin(var)`, `1/var`, `var^var`, etc. | Return with `NotPolyKind` payload; surface ops re-raise to user |
| `PolyError::ExponentOverflow` | `pow` with `k > u32::MAX`; `expand` with exponent above `EXPAND_POW_LIMIT`; `view_product` exponent sum overflow | Return; user sees a clear "exponent too large" error |

The polynomial engine never panics. Internal invariant violations (e.g. unsorted term
list after a merge) are caught by `debug_assert!` in debug builds and become benign
no-ops in release builds — `to_expr` re-sorts defensively before emission.

The `Span` field on errors is populated when a `view` failure can be traced back to a
specific subterm; the parser's `SpanMap` is consulted by `surface.rs` before surfacing
the error to Python (where `PolyError` maps to `PolyError(monomix.errors)` with an
optional source-location field).

---

## 4. Trade-off Analysis

### 4.1 Sparse term-list vs. dense coefficient vector

**Chosen: sparse `(exponent, coefficient)` term list, sorted descending by exponent.**

| Approach | Memory | `add`/`sub` | `mul` | Notes |
|----------|--------|-------------|-------|-------|
| Sparse term list (chosen) | O(non-zero terms) | O(n+m) merge | O(n·m·log min) heap | Wins on typical CAS inputs where polynomials are sparse |
| Dense coefficient vector | O(degree) | O(max(deg)) | O(deg²) or O(deg log deg) FFT | Wins on dense small-degree polynomials (rare in CAS) |
| Recursive multivariate (REDUCE SF) | O(distinct power products) | O(structure) | O(structure²) | Phase 2 multivariate path; overkill for univariate |

**Why sparse wins for Phase 1.** User-written polynomials are overwhelmingly sparse —
`x^100 + 1` has 2 terms, not 101. Even post-`expand`, the term count is bounded by the
expansion structure (binomial coefficients) and is far smaller than the degree for
generic inputs. Dense storage spends most of its memory on zeros, and the `mul` path's
O(deg²) wins over O(n·m) only when the polynomials are *both* dense — a rare case in
symbolic computation.

**Revisit trigger.** If profiling shows `mul` dominating runtime on dense-polynomial
benchmarks, evaluate FFT/NTT-based multiplication. The §5.1 migration is a single-
function swap because the public API takes `&UnivPoly` and returns `UnivPoly` —
internal storage of the terms is encapsulated.

### 4.2 ExprId coefficients vs. structured coefficient type

**Chosen: coefficients are `ExprId`s, not a structured `Coefficient` enum.**

REDUCE's standard form uses a recursive structure: `Polynomial<Polynomial<...<Q>>>` to
represent multivariate, with each level keyed by a different variable. The leaf is a
canonical numeric type. This gives static guarantees about coefficient shape but
explodes type complexity and locks the engine into a fixed coefficient algebra.

`ExprId` coefficients are dynamically typed: a coefficient might be a rational, a
sum, a product, or a more complex expression. The benefits:

1. **No double-canonicalization.** A coefficient that arose from a previous simplifier
   call already has its canonical-form invariants from the pool; we don't re-canonicalize
   when storing it in a `Term`.
2. **Subexpression sharing.** Two terms with the same coefficient share an `ExprId` —
   `2*x + 2*y` stores one `pool.integer(2)` handle in both terms.
3. **Phase-2 friendly.** Multivariate polynomials need coefficients that themselves
   contain other variables; `ExprId` already covers this without a recursive type.
4. **Symbolic-coefficient support is free.** `(y+1)*x + 1` works in `view` without any
   special case — the coefficient `(y+1)` is just an `ExprId`.

The cost is that arithmetic on coefficients goes through `pool.add` / `pool.mul`, which
is slower than direct rational arithmetic. The benchmark target (50-term polynomial in
<100 ms) is met because the simplifier (`designs/simplifier.md` §3.2-3.3) handles
numeric folding *before* the polynomial engine sees the input — the pool constructors
return cached handles for already-folded coefficients in the common case.

### 4.3 Heap-driven sparse multiplication vs. distributive expansion

**Chosen: heap-merge sparse convolution (Monagan-Pearce).**

| Approach | Time | Memory | Output canonical? | Notes |
|----------|------|--------|-------------------|-------|
| Heap-merge (chosen) | O(n·m·log min(n,m)) | O(min(n,m)) | Yes (descending order) | One sweep, no re-sort |
| Hashmap accumulate then sort | O(n·m + k·log k) | O(k) where k = distinct exponents | No (sort needed) | Faster for very-sparse outputs |
| Schoolbook nested loops | O(n·m) | O(n·m) before merge | No (sort + dedup needed) | Simplest but allocates the full product list |

Heap-merge has the desirable property that the output emerges in canonical order, so
there's no separate sort. It also has tight memory: the heap holds at most `n` entries
(one per term of the left operand), independent of the output size. Hashmap-based
accumulation is faster when many `(i,j)` pairs collide on the same exponent (so the
output is much smaller than n·m), but in CAS workloads the typical multiplication is
between two sparse polynomials with few coincidences.

The implementation cost is moderate — a heap and an index-tracking array — and the
algorithm is well-understood, with reference implementations in Maple, Singular, and
Pari/GP to cross-check against.

### 4.4 Schoolbook division vs. fast division

**Chosen: schoolbook long division.**

Newton-iteration-based fast division gives `O((deg a) log(deg a))` for division-with-
remainder, vs schoolbook's `O((deg a − deg b) · deg b)`. The crossover is around
degree 100 for dense polynomials.

Phase 1 polynomials are typically degree ≤ 20 (the `df` benchmark target). At that size,
schoolbook is faster — Newton division has a fixed setup cost (computing the
denominator's reciprocal series to sufficient precision) that schoolbook avoids. Phase
2's factorization (SCOPE.md §2.2) is where fast division earns its complexity, because
square-free decomposition and Berlekamp's algorithm both require many divisions on
larger polynomials.

The migration path is identical to the multiplication case: `divide` is a single
function whose internal algorithm can be swapped without API change. The §6.3 benchmark
suite includes division on degree-50 inputs as a regression guard, so the migration
trigger is mechanical: when those benchmarks regress past the target, switch.

### 4.5 GCD-on-every-op vs. GCD-on-opt-in

**Chosen: GCD opt-in via `SimplifierConfig::gcd`, default off.**

REDUCE's `simp` defaults to `off gcd`
([polrep.red:54-67](../legacy/reduce-algebra-code-r7357-trunk/packages/poly/polrep.red))
because full polynomial GCD on every `Add`/`Mul` of `Div`s dominates the cost of
symbolic-heavy workloads. We match the default, with explicit opt-in for users who want
fully-reduced fractions.

The cost of the default-off setting is that some users will see "non-reduced" fractions
in output (`(x+1)*(x-1) / (x-1)` displays as `(x^2-1)/(x-1)` rather than `x+1`). This is
the right trade-off: the polynomial engine still cancels exact divisors on `simplify_div`
(via the schoolbook division path with zero remainder), so the common case `x^2/x → x`
works without GCD. Only the cases where exact division leaves a non-zero remainder
require true GCD to fully reduce, and those cases are precisely where the cost is high.

The config switch `SimplifierConfig::gcd` is the lever; users who want maximal
canonicalization enable it on the `Session` and accept the latency. This matches the
REDUCE workflow and the discussed defaults table (§1.4).

### 4.6 Architectural divergence from REDUCE — `UnivPoly` vs. `SF/SQ`

This design's relationship to original REDUCE deserves to be named explicitly so that
golden-corpus reviewers and future maintainers understand the structural choice.

**REDUCE's model.** REDUCE's `simp` produces a **standard quotient (SQ)**:
`numerator/denominator` where each is a sparse recursive multivariate polynomial in
canonical normal form
([polrep.red:45-68](../legacy/reduce-algebra-code-r7357-trunk/packages/poly/polrep.red)).
Every kernel operation (`addsq`, `multsq`, `simpexpt`, ...) takes SQ inputs and produces
a fully canonical SQ output. There is no separate "polynomial type" — polynomials are
just SQs with `denominator = 1`.

**This design's model.** A general expression DAG (`ExprNode` variants) with a separate
`UnivPoly` value type that is *constructed on demand* from `ExprId` via `view`. The
polynomial engine operates on `UnivPoly`; users and the simplifier see only `ExprId`.

**Implications:**

| Feature | REDUCE (SF/SQ-form) | This design (DAG + UnivPoly view) |
|---|---|---|
| Polynomial recognition | Implicit in the data structure | Explicit `view` call (§3.2) |
| Multivariate support | Free (recursive type) | Phase 2 work (§5.1) |
| Coefficient algebra | Forced through SQ recursion | Local — coefficient is any `ExprId` |
| Symbolic coefficients | Supported via "kernels" | First-class via `ExprId` |
| Re-canonicalization cost | Forced on every op | Opt-in via `simplify` |
| Sub-expression sharing | Lost across SQ boundaries | Hash-cons gives O(1) sharing |
| Surface display forms | Re-derived from SQ each time | Free — walk the DAG |

**Why the DAG + UnivPoly model is the right choice for Monomix:**

1. **Structure preservation.** A user-typed `(a+b)^2 / c` stays as `Pow(Add, 2) / c`
   until `expand` is called. REDUCE's SQ form would have already distributed the power.
2. **Lazy polynomial typing.** A `Mul` node only becomes a `UnivPoly` when something
   needs it to be one (the simplifier's `simplify_div`, the user's `expand`/`collect`).
   Most expressions never go through `view` at all.
3. **Multi-purpose engine.** The same `UnivPoly` arithmetic that backs `simplify_div`
   also backs `expand`, `collect`, `deg`, and `coeff`, and will back the quadratic
   solver in Phase 1 §1.6 (which needs coefficient extraction from `a*x^2 + b*x + c`).

**Implications for testing.** The §6.5 golden-corpus must accept that REDUCE's canonical
output is one form among many that are equally "correct"; the test harness records
intentional divergences — for example, REDUCE's `(x^2-1)/(x-1)` reduces to `x+1` under
default `mcd=on`, while ours leaves it as `(x^2-1)/(x-1)` under default `gcd=off,
mcd=off` — in a manifest with `# reason: ...` annotations.

**Revisit trigger.** If the cost of `view` becomes a profile hot spot, evaluate caching
the `(ExprId, var) → UnivPoly` mapping on the `Session` (similar to `SimplifyCache`).
The engine's API doesn't change; only the surface entry points wrap the cache lookup.

---

## 5. Scale, Limits, and Future Work

### 5.1 Phase 2: Multivariate polynomials (SCOPE.md §2.5)

Phase 2 generalizes `UnivPoly` to `MultiPoly` with a configurable monomial order. The
groundwork in Phase 1:

- Public surface (`expand`, `collect`, `deg`, `coeff`) takes `var: Symbol` so adding
  multivariate variants (`expand_multi`, etc.) is non-breaking. The single-variable
  versions remain as convenience wrappers.
- `view` returns `Result<UnivPoly, PolyError>` with `Multivariate { other_var }` as a
  failure mode. Phase 2 changes this from "failure" to "fall through to multivariate
  view"; the simplifier's call sites already handle the failure path by leaving
  expressions unchanged.
- `UnivPoly`'s sparse `(exponent, coefficient)` list generalizes to
  `(monomial_exponent_vector, coefficient)`. The arithmetic algorithms (heap-merge `mul`,
  schoolbook `divide`) lift directly with the monomial order driving the heap key.
- `gcd_univariate` is replaced by a multivariate GCD (Buchberger / S-polynomial-free
  variants) — but this is in the larger §2.6 package.

Phase 2 work concentrates on:

- **Monomial order parameterization.** Lex by default per SCOPE.md §2.5; the API takes
  an `Order` trait object so users can request grevlex.
- **Subresultant GCD.** Multivariate GCD is the prerequisite for factorization (§2.2).
- **Performance work.** Multivariate multiplication amplifies the heap-merge benefits;
  the same algorithm scales but the constant factors are higher.

### 5.2 Phase 2: Polynomial factorization (SCOPE.md §2.2)

Square-free factorization, Berlekamp mod p, Hensel lifting. All three live in `gcd.rs`'s
descendant — Phase 2 will likely split it into `gcd.rs`, `squarefree.rs`,
`berlekamp.rs`, and `hensel.rs`. Phase 1's content/primitive-part GCD is the foundation;
the algorithmic work is purely additive.

### 5.3 Phase 3+: Fast multiplication and division

When Phase 2 multivariate work pushes degrees into the 100-1000 range, FFT-based
multiplication and Newton-iteration-based division become worthwhile. The `mul` and
`divide` functions in `ops.rs` are single points of substitution for both algorithms;
the public API does not change. The trigger is benchmark-driven (§6.3): if the
"degree-100 multiplication" benchmark regresses past target, switch.

NTT modulus selection is the substantive design work — small primes for fast modular
arithmetic, with CRT recombination, as in Maple's modular polynomial arithmetic. None
of this is scoped for Phase 1 or Phase 2.

### 5.4 Result caching across operations

Phase 2's MCP cache (SCOPE.md §2.8) could cache `expand(ExprId) → ExprId` and
`view(ExprId, var) → UnivPoly` mappings, since both are pure functions of their inputs.
Migration path is the same as the simplifier's: once `ExprId` is content-addressed
(`designs/expression-dag.md` §5.4), cache entries become hash → result pairs.

The `view` cache is the more interesting one — `view` is currently called once per
`simplify_div` invocation, and a cache hit on a repeated input is a clean win. The
design assumption is that the cache lives on the `Session`, capped at some reasonable
entry count, with the same full-clear eviction strategy as `SimplifyCache`.

### 5.5 Symbolic exponents and rational-function fields

Phase 1 deliberately rejects `Pow(var, e)` where `e` is non-natural-integer. Phase 2's
advanced simplifier (§2.6) may want a separate "rational-function field" view that
treats `var` as a transcendental and allows negative exponents. This is out of scope
for the polynomial engine itself; it would be a sibling type (`RationalFunc { num:
UnivPoly, den: UnivPoly }`) with its own arithmetic.

---

## 6. Testing Strategy

### 6.1 Unit tests (`cargo test`)

**Sparse representation invariants:**
- `view(2*x^3 + x^3 + 5)` produces a single term `(3, 3)` plus a constant — duplicate
  exponents collapse during `view`.
- `view(x - x)` produces the empty term list (`is_zero`).
- `view(0)` produces the empty term list with the variable still recorded.

**`view` recognition:**
- `view(sin(x), x)` returns `Err(NotPolynomial { kind: NonPolynomialFunction(Sin) })`.
- `view(1/x, x)` returns `Err(NotPolynomial { kind: InDenominator })`.
- `view(x^x, x)` returns `Err(NotPolynomial { kind: SelfReferential })`.
- `view(x^(-1), x)` returns `Err(NotPolynomial { kind: NonNaturalExponent })`.
- `view((y+1)*x^2 + x, x)` succeeds with terms `(2, y+1)` and `(1, 1)`.
- `view(x^2 + y, x)` returns `Err(Multivariate { other_var: y })`.

**Arithmetic — numeric coefficients:**
- `add(x + 1, 2*x + 3) == 3*x + 4`.
- `sub(x^2 + x, x^2) == x`.
- `mul((x+1), (x-1)) == x^2 - 1`.
- `pow((x+1), 5)` produces a 6-term expansion with the correct binomial coefficients.
- `divide(x^3 - 1, x - 1) == (x^2 + x + 1, 0)` (exact division).
- `divide(x^2 + 1, x) == (x, 1)` (with non-zero remainder).
- `divide(0, x) == (0, 0)` (zero numerator).
- `divide(x, 0)` returns `Err(DivisionByZero)`.

**Arithmetic — symbolic coefficients:**
- `mul((y+1)*x, x + 1) == (y+1)*x^2 + (y+1)*x`.
- `add((y+1)*x, (y-1)*x) == 2*y*x` (after the simplifier folds the coefficient sum;
  if no simplifier present, accept `((y+1) + (y-1))*x` and re-route through `simplify`).

**Surface operations:**
- `expand((x+1)^3) == x^3 + 3*x^2 + 3*x + 1`.
- `expand((a+b)*(c+d)) == a*c + a*d + b*c + b*d`.
- `expand((a+b)^EXPAND_POW_LIMIT)` returns `Err(ExponentOverflow)`.
- `collect(x*y + x^2*z + x*w, x) == (y+w)*x + z*x^2`.
- `deg(3*x^7 + x^2 + 5, x) == 7`.
- `deg(0, x) == 0` (REDUCE convention).
- `coeff(3*x^2 + 5*x + 2, x, 1) == 5`.
- `coeff(3*x^2, x, 0) == 0`.
- `coeff(3*x^2, x, 5) == 0` (missing exponents return zero).

**`is_polynomial_in`:**
- `is_polynomial_in(x^2 + sin(y), x) == true` (sin is in another variable).
- `is_polynomial_in(x^2 + sin(y), y) == false`.
- `is_polynomial_in(x^x, x) == false`.

**`common_univariate`:**
- `common_univariate(x^2 + 1, x - 1) == Some(x)`.
- `common_univariate(x + y, x - y) == None` (multivariate on each side).
- `common_univariate(x + 1, y + 1) == None` (different variables).

**Idempotence regression:**
- `expand(expand(e)) == expand(e)` for each test expression.
- `collect(collect(e, x), x) == collect(e, x)`.
- `to_expr(view(to_expr(view(e, x)), x)) == to_expr(view(e, x))` (round-trip stability).

### 6.2 Property-based tests (`proptest`)

- **Quotient-remainder invariant** (the load-bearing test for `divide`): for randomly
  generated `a, b` with `b ≠ 0`, the result `(q, r) = divide(a, b)` satisfies
  `simplify(a - (q*b + r)) == 0` and `deg(r) < deg(b)`.
- **Multiplication commutativity:** `mul(a, b) == mul(b, a)` structurally.
- **Distributivity:** `mul(a, add(b, c)) == add(mul(a, b), mul(a, c))` structurally.
- **`expand` idempotence** (SCOPE.md §1.12): `expand(expand(e)) == expand(e)`.
- **`expand`/`simplify` round-trip** (SCOPE.md §1.12): `simplify(expand(e)) == simplify(e)`
  on the equivalence class of expressions; checked by numerical agreement on random
  bindings (rational evaluation, exact equality).
- **No spurious zero terms:** for random inputs, the output's `terms` list contains no
  `coefficient == pool.zero` entries (Invariant 2 in §3.1).
- **Sorted output:** for random inputs, the output's `terms[i].exponent > terms[i+1].
  exponent` (Invariant 1 in §3.1).
- **`pow(a, 0) == 1`** for any `a` (including the zero polynomial — degenerate case but
  matches the conventional `0^0 = 1` for polynomial exponents; document as REDUCE-
  compatible).
- **`mul` commutes with `view`:** for random `e1, e2` polynomial in the same variable,
  `view(simplify(mul_expr(e1, e2)), var) == mul(view(e1, var), view(e2, var))` (the
  engine and the pool's mul agree on what the product is).

### 6.3 Benchmarks (`criterion`)

| Benchmark | Target |
|-----------|--------|
| `expand((x+1)^10)` | <50 ms (SCOPE.md §1, derived from the 50-term sum target) |
| `expand((x+1)^20)` | <200 ms (regression guard for `pow` exponent doubling) |
| `mul` of two 50-term polynomials | <20 ms |
| `divide` of degree-50 / degree-25 | <50 ms (Phase 2 fast-division trigger) |
| `view` of an already-canonical 1k-node polynomial | <5 ms (regression guard) |
| `coeff` extraction from a 100-term polynomial | <1 ms |
| `gcd_univariate` of two degree-20 polynomials with rational coefficients | <100 ms |

The "view of an already-canonical 1k-node polynomial" benchmark is the regression guard
for the `view` walk — it should be linear in the input size and have minimal allocation
overhead. If it regresses past target, the bucket-map logic has likely grown a quadratic.

### 6.4 Fuzz testing (`cargo-fuzz`)

- **Target:** `expand(parse(arbitrary_bytes))`. Asserts (a) no panics, (b)
  `expand(expand(e)) == expand(e)`, (c) `EXPAND_POW_LIMIT` is honoured (returns Err
  rather than running unboundedly), (d) the output pool's `len()` is bounded by some
  reasonable multiple of the input.
- **Target:** `divide(parse(a), parse(b))` for random `a, b`. Asserts (a) no panics,
  (b) the quotient-remainder invariant `simplify(a - (q*b + r)) == 0` holds when
  `divide` returns `Ok`, (c) `DivisionByZero` is returned when `b` simplifies to zero.
- **Seed corpus:** the legacy `.tst` files (curated subset that parses cleanly under
  the Phase 1 grammar) plus a small hand-curated set of pathological inputs (very
  high exponents, deeply-nested products, polynomials with many small-coefficient
  terms).
- **Run duration:** ≥1 hour per release (combined with the parser and simplifier fuzz
  targets).

### 6.5 Golden-corpus tests (`pytest`)

A subset of `legacy/reduce-algebra-code-r7357-trunk/packages/poly/*.{tst,rlg}` and
`packages/alg/*.{tst,rlg}` (the ones exercising `expand`, `collect`, `deg`, `coeff`,
and division). For each `.tst` input, parse, run the corresponding operation, and
compare against the `.rlg` output.

**Known intentional divergences from REDUCE** (recorded in the manifest with
`# reason: ...` annotations, not treated as failures):

- `(x^2 - 1) / (x - 1)` not auto-reduced to `x + 1` — REDUCE's `mcd` is on by default;
  ours is off (see §1.4). Test should invoke `simplify` with `cfg.gcd = true` (or
  invoke the polynomial engine's `divide` directly) to match.
- `(a+b)^2` not auto-expanded — REDUCE's `expandexpt` is on by default; ours is off
  for `simplify` (see §1.4 and `designs/simplifier.md` §1.4). The user can call
  `expand` explicitly.
- Polynomial GCD output canonicality — REDUCE normalizes GCD leading coefficients to
  positive; ours leaves the leading coefficient as-emitted unless it is a recognized
  numeric. Document case-by-case.
- `coeff(3*x^2, x, 5) == 0` — matches REDUCE.
- `deg(0, x) == 0` — matches REDUCE.

The curated set lives in `tests/golden/poly/` with the manifest mapping input file to
expected output and the `# reason: ...` annotation per case.

---

## 7. Action Items

### Phase 1 — Core implementation

1. [ ] Create `crates/monomix-kernel/src/poly/mod.rs` exposing the public API (§2.1);
       wire `PolyError` into `KernelError` with `NotPolynomial`, `DivisionByZero`,
       `Multivariate`, `ExponentOverflow` variants
2. [ ] Define `UnivPoly`, `Term`, and the invariants in `repr.rs` (§3.1) with
       `debug_assert!`-driven invariant checks
3. [ ] Implement `view.rs` — recursive walker producing `Result<UnivPoly, PolyError>`,
       hashmap-based bucket coalescing, structured error reporting (§3.2)
4. [ ] Implement `emit.rs` — `to_expr` re-emission through pool constructors with the
       1/0/k cases handled (§3.4)
5. [ ] Implement `ops.rs` — two-pointer merge `add`/`sub`, heap-merge `mul`,
       repeated-squaring `pow`, schoolbook `divide` (§3.3)
6. [ ] Implement `surface.rs` — `expand`, `collect`, `deg`, `coeff`,
       `is_polynomial_in`, `common_univariate`, `EXPAND_POW_LIMIT` defensive cap (§3.5)
7. [ ] Implement `gcd.rs` — content/primitive-part Euclidean GCD; gated behind the
       simplifier's `cfg.gcd` switch and not invoked by the engine itself (§3.6)
8. [ ] Wire `expand` and `collect` into the Python `Session` via PyO3 with the same
       `subtree_size > 500` GIL-release threshold as the simplifier (§3.8)
9. [ ] Coordinate with `designs/simplifier.md` §3.5 on the `simplify_div` integration —
       the simplifier calls `view + divide + to_expr` and uses `PolyError::Multivariate`
       and `PolyError::NotPolynomial` to decide when to give up gracefully

### Phase 1 — Verification

10. [ ] Unit-test all transformations enumerated in §6.1, including the structured-error
        paths and the symbolic-coefficient cases
11. [ ] `proptest` quotient-remainder invariant + idempotence + commutativity +
        distributivity (§6.2)
12. [ ] `criterion` benchmarks including the "view of already-canonical 1k-node"
        regression guard (§6.3)
13. [ ] `cargo-fuzz` target with `expand` idempotence + `EXPAND_POW_LIMIT` enforcement +
        `divide` quotient-remainder invariant (§6.4)
14. [ ] Curate the golden-corpus `.tst`/`.rlg` subset for polynomial operations, with a
        divergence manifest covering the intentional divergences in §6.5
15. [ ] Confirm SCOPE.md §1.12 invariants hold: `expand` idempotence, `expand` ∘
        `simplify` round-trip

### Phase 2 — Generalization (deferred)

16. [ ] Generalize `UnivPoly` to `MultiPoly` with monomial-order parameter; lift the
        arithmetic and surface operations (SCOPE.md §2.5)
17. [ ] Replace `view`'s `Multivariate` failure with a multivariate-aware view path;
        update simplifier call sites to use the new path
18. [ ] Implement square-free factorization, Berlekamp mod p, and Hensel lifting in
        `squarefree.rs` / `berlekamp.rs` / `hensel.rs` (SCOPE.md §2.2)
19. [ ] Add subresultant GCD as a faster alternative to content/primitive-part on the
        rational-coefficient path (§3.6 caveat 1)
20. [ ] Add a `view` cache on `Session` once `ExprId` is content-addressed
        (`designs/expression-dag.md` §5.4); follow the `SimplifyCache` design (§5.4)
21. [ ] Implement FFT/NTT-based multiplication when degree-100 benchmarks regress past
        target (§5.3)
22. [ ] Implement Newton-iteration-based fast division alongside FFT multiplication
23. [ ] Add a `RationalFunc` sibling type for the rational-function field if Phase 2's
        advanced simplifier (§2.6) needs it (§5.5)
