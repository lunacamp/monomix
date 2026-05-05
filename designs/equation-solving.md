# Equation Solving — System Design

**Component:** `monomix-kernel::solve`
**Status:** Design phase
**Date:** 2026-05-03
**References:** SCOPE.md §1.6, §1.5, §1.7, §1.4, §0.4, §0.7; ADR-0001; ADR-0002; `designs/expression-dag.md`; `designs/parser.md`; `designs/simplifier.md`; `designs/polynomial-ops.md`

---

## 1. Requirements

### 1.1 Functional requirements

The solver is the kernel's equation-solving engine. It is the implementation of
SCOPE.md §1.6 and is consumed by the `solve(...)` surface function exposed to the Python
`Session` and the parser's `solve` builtin (`designs/parser.md` §3.3).

It must support the full surface listed in SCOPE.md §1.6:

- **Single linear equation.** `a*x + b = 0` ⟹ `x = -b/a`. The coefficients `a`, `b` may
  be any `ExprId` not containing the unknown — they need not be numeric.
- **Single quadratic equation** via the quadratic formula:
  `a*x² + b*x + c = 0` ⟹ `x = (-b ± √(b² − 4·a·c)) / (2·a)`. When `a` is provably
  numeric and the discriminant is provably negative, the solver emits the empty solution
  set with a `MonomixWarning` rather than a complex root (SCOPE.md §1.6, "Behavior on no
  real solutions").
- **Polynomial special forms** that reduce to linear or quadratic via degree analysis or
  simple factoring: `x² − 9 = 0`, `x² = 4`, `(x − 3)·(x + 7) = 0`, `2·x = 6`. The
  recognizer leans on `poly::view` (`designs/polynomial-ops.md` §3.2) for the shape
  classification.
- **`n × n` linear systems** via Gaussian elimination with partial pivoting. The input
  is a list of `n` equations in `n` named unknowns; the output is a single substitution
  binding each unknown to a closed-form expression in the others' coefficients.
- **Empty solution set** (`{}`) with a `MonomixWarning("no real solutions; complex roots
  not supported until Phase 3")` for `x² + 1 = 0` and other equations whose only roots
  are complex. The warning is raised once per `solve` call, not once per missed root.
- **`UnsupportedError`** with the message `"equation form not supported"` (extended with
  a structured payload — see §3.9) for cubic, quartic, transcendental, exponential, and
  trigonometric equations. SCOPE.md §1.6 explicitly defers these to Phase 3+.

The result of every successful `solve` call is a `SolutionSet`: an ordered list of
zero-or-more substitutions, where each substitution maps each declared unknown to an
`ExprId`. Empty list ⟺ no real solutions; one substitution ⟺ a unique solution; more
than one ⟺ multiple roots (the typical case for quadratics).

### 1.2 Non-functional requirements

- **Bounded time.** The solver finishes in time linear in the size of its input — there
  is no fixed-point loop, no recursive simplifier callback that could re-enter the
  solver. The single quadratic and the `n × n` system both have known closed-form
  algorithms with polynomial complexity in `n` and the input expression sizes.
- **No panics.** All errors are returned as `KernelError` variants and surface in
  Python as subclasses of `monomix.MonomixError` (SCOPE.md §0.4).
- **Determinism.** For the same input expression IDs and the same ordering of unknowns,
  the solver returns the same `SolutionSet` byte-for-byte. Hash-based pivot selection is
  out — the `partial pivoting` algorithm uses a deterministic tie-breaker (smallest row
  index among rows tied on the pivot magnitude).
- **Idempotent under simplification.** Each output `ExprId` is simplified through
  `simplify` (`designs/simplifier.md` §2.1) before return. A second `solve` of the same
  input produces structurally identical output (same `ExprId`s, given a stable cache).
- **No allocation in the trivial path.** Solving a constant equation (`5 = 5`, all
  variables, true ⟹ "all reals", typically) is detected at the front of the driver and
  returns an empty / sentinel solution set without entering the polynomial engine.

### 1.3 Constraints

- **Real-only output.** SCOPE.md §1.6 commits Phase 1 to real solutions. Complex roots
  are an explicit Phase 3+ deliverable (SCOPE.md §3.1). The "no real solutions" warning
  is the contract; the solver does not silently produce `√(−1)` or imaginary expressions.
- **No symbolic exponents in unknowns.** The solver treats the unknown(s) as kernel
  symbols. `x^a` where `a` is a symbol is rejected as a non-polynomial form, matching
  the polynomial engine's stance (`designs/polynomial-ops.md` §3.7).
- **Finite degree.** The polynomial engine's `EXPAND_POW_LIMIT = 100` is the upstream
  cap on input expansion (`designs/polynomial-ops.md` §3.5); the solver does not invoke
  `expand` itself but inherits the cap when callers pre-expand.
- **One unknown per single equation; `n` unknowns per `n` equations for systems.**
  The solver does not handle under-determined or over-determined systems in Phase 1 —
  these surface as `UnsupportedError` with a structured reason. Parametric solutions
  with introduced free symbols are explicitly Phase 2+ (out of scope for §1.6).
- **The solver is not a simplifier.** It calls `simplify` on its outputs before
  returning, but the simplifier flag set is the caller's session-level config — the
  solver does not flip switches mid-call.

### 1.4 What the solver is **not**

To pin scope precisely:

- It is **not the parser.** The parser handles `solve(eq, x)` and `solve({eq1, eq2},
  {x, y})` syntax and lowers it (`designs/parser.md` §2.3, §3.3). The solver receives
  already-parsed `ExprId`s and `Symbol`s.
- It is **not the polynomial engine.** It depends on `poly::view`, `poly::deg`, and
  `poly::coeff` (`designs/polynomial-ops.md` §2.1, §3.2, §3.5) but does not implement
  polynomial arithmetic itself. Coefficient extraction is the engine's job; closed-form
  root computation is the solver's job.
- It is **not the simplifier.** It calls `simplify` on its outputs but does not
  rearrange the input equation through identity matching. `solve(sin(x)² + cos(x)² = 1,
  x)` is not satisfied by Phase 1 — the simplifier's `simplify_trig` could collapse the
  LHS to `1` but only if invoked first by the user, after which the solver sees
  `1 = 1`, reports an "always true" sentinel, and the call succeeds with the
  "all-reals" result.
- It is **not a numeric solver.** No bisection, Newton, or interval arithmetic. All
  Phase 1 roots are closed-form symbolic. Numeric refinement of symbolic roots is a
  Phase 2 candidate (§5).
- It does **not** introduce free parameters for under-determined systems, attempt
  Gröbner-basis reduction for non-linear systems, or perform variable elimination
  across symbolic domains. All Phase 2+ (SCOPE.md §2.5, §2.6, §3.2).

---

## 2. High-Level Design

### 2.1 Public API

```rust
/// Solve a single equation in a single unknown. Returns a SolutionSet — possibly
/// empty (no real roots), with one entry (linear), or with two (quadratic). The
/// order of the substitutions in the SolutionSet is stable and documented per
/// algorithm in §3.
///
/// `eq` may be an `Eq(lhs, rhs)` node or a bare expression (interpreted as `expr = 0`).
/// `var` must be a `Symbol` ExprId.
///
/// Errors:
/// - `KernelError::UnsupportedEquationForm { reason }` — degree > 2, transcendental,
///   non-polynomial in `var`, or any shape SCOPE.md §1.6 explicitly defers.
/// - `KernelError::NotASymbol` — `var` is not a Symbol.
/// - Any `KernelError` propagated by the simplifier or polynomial engine.
pub fn solve(
    pool: &mut ExprPool,
    cfg: &SolverConfig,
    eq: ExprId,
    var: ExprId,
) -> Result<SolutionSet, KernelError>;

/// Solve a system of `n` equations in `n` unknowns. Variable order in `vars`
/// determines the ordering of bindings in each result substitution. The system is
/// solved by Gaussian elimination with partial pivoting; non-linear systems are
/// rejected with `UnsupportedEquationForm { reason: NonLinearSystem }`.
///
/// Returns either zero substitutions (inconsistent system, e.g. `x = 1, x = 2`) or
/// exactly one substitution (consistent, exactly determined). Under-determined and
/// over-determined systems are rejected (Phase 1 scope cut).
pub fn solve_system(
    pool: &mut ExprPool,
    cfg: &SolverConfig,
    eqs: &[ExprId],
    vars: &[ExprId],
) -> Result<SolutionSet, KernelError>;

/// One root of a single equation, or one assignment in a system. Maps each declared
/// unknown to a closed-form ExprId. Always contains exactly the unknowns the caller
/// supplied — never extra symbols, never missing entries.
#[derive(Clone, Debug)]
pub struct Substitution {
    /// Indexed in the same order as the `vars` argument to the call.
    pub bindings: SmallVec<[(Symbol, ExprId); 4]>,
}

/// Zero-or-more substitutions plus a structured reason when the set is empty.
#[derive(Clone, Debug)]
pub struct SolutionSet {
    pub substitutions: SmallVec<[Substitution; 2]>,
    /// `None` when the set is non-empty. `Some(NoRealRoots { discriminant: ExprId })`
    /// when a quadratic was provably real-rootless and the empty set is a real
    /// answer rather than a degenerate one. The PyO3 boundary uses this to decide
    /// whether to emit the SCOPE.md §1.6 `MonomixWarning`.
    pub empty_reason: Option<EmptyReason>,
}

#[derive(Clone, Debug)]
pub enum EmptyReason {
    /// Quadratic with provably negative numeric discriminant; complex roots only.
    NoRealRoots { discriminant: ExprId },
    /// Linear system reduced to `0 = c` for some non-zero `c`.
    Inconsistent { row: usize, residual: ExprId },
    /// Always-true equation (e.g. `0 = 0`); the solver reports this as the empty
    /// substitution list with a non-error reason. Caller must distinguish "no
    /// solutions" from "all solutions" — the parser binding emits a separate
    /// `IdentityHolds` warning rather than a value.
    AllSolutions,
}

/// Configuration mirroring the simplifier's `SimplifierConfig` discipline. The
/// solver's behaviour is deterministic and the only knobs are surfaces that mirror
/// REDUCE switches relevant to solving. Defaults match §1.4 of the overall design.
#[derive(Clone, Debug, Default)]
pub struct SolverConfig {
    /// `true` (default): run `simplify` on each output ExprId before returning.
    /// `false`: leave outputs as raw closed-form expressions (used by the test
    /// harness to check raw algebraic shape).
    pub simplify_outputs: bool,
    /// `true` (default): use partial pivoting in `solve_system`. `false`: take the
    /// first non-zero coefficient per column. Disabling is for a regression test only.
    pub partial_pivot: bool,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum SolverError {
    #[error("equation form not supported: {reason}")]
    UnsupportedEquationForm { reason: UnsupportedReason, span: Option<Span> },
    #[error("variable {0:?} is not a symbol")]
    NotASymbol(ExprId),
    #[error("system has {n_eqs} equations and {n_vars} unknowns; Phase 1 requires n×n")]
    NonSquareSystem { n_eqs: usize, n_vars: usize },
}

#[derive(Clone, Debug)]
pub enum UnsupportedReason {
    /// Polynomial of degree ≥ 3 in the unknown.
    DegreeTooHigh { degree: u32 },
    /// Equation is not polynomial in the unknown — `sin(x) = 1`, `e^x = 2`, `1/x = 0`.
    NonPolynomial { kind: poly::NotPolyKind },
    /// `solve_system` saw an equation that is not linear in at least one unknown.
    NonLinearSystem { equation_index: usize },
    /// Caller passed a vars list with duplicates.
    DuplicateUnknowns,
}
```

The `SolverError` enum is structured so the parser's `SpanMap` (`designs/parser.md` §3.5)
can attach a source span to each variant — the `span: Option<Span>` field is populated
when the originating subterm came from a parsed input. The `UnsupportedReason` payload
is what the Python boundary uses to format the user-facing message; the bare string
`"equation form not supported"` is the SCOPE.md-mandated prefix, with the structured
reason appended ("…degree 3", "…transcendental", "…non-linear system at equation 2").

`SolutionSet::empty_reason` is the contract that powers the SCOPE.md §1.6 warning: the
PyO3 boundary inspects `Some(NoRealRoots { .. })` and emits the `MonomixWarning` exactly
once, with the discriminant value attached for diagnostics. `Inconsistent` and
`AllSolutions` are *not* warnings — they are valid algebraic answers.

### 2.2 Component diagram

```
                  ExprId (eq) + Symbol (var)            ExprId list (eqs) + Symbol list (vars)
                            │                                       │
                            ▼                                       ▼
                  ┌─────────────────────┐                 ┌─────────────────────┐
                  │  driver_single.rs   │                 │  driver_system.rs   │
                  │  (normalize + degree│                 │  (build matrix +    │
                  │   dispatch)         │                 │   eliminate)        │
                  └──────────┬──────────┘                 └──────────┬──────────┘
                             │                                       │
                             │ poly::view (NotPoly → fail)           │ poly::view per row
                             │ poly::deg                             │ poly::coeff per (row, var)
                             ▼                                       ▼
   ┌─────────────────────────────────────────────────┐    ┌─────────────────────────────┐
   │                                                 │    │                             │
   ▼            ▼                ▼            ▼      ▼    ▼              ▼              ▼
┌─────────┐ ┌─────────┐ ┌────────────────┐ ┌─────────────┐ ┌──────────────────────┐
│constant │ │ linear  │ │ quadratic      │ │ unsupported │ │ gaussian elimination  │
│ (0=0,   │ │ (-b/a)  │ │ (-b±√D)/(2a)  │ │ (degree>2)  │ │ + back substitution   │
│  c≠0)   │ │         │ │ + discriminant │ │             │ │ + partial pivoting    │
│         │ │         │ │   sign check   │ │             │ │                       │
└────┬────┘ └────┬────┘ └────┬───────────┘ └──────┬──────┘ └───────┬───────────────┘
     │           │            │                    │                │
     └───────────┴────────────┴────────────────────┴────────────────┘
                            │
                            ▼
                  ┌─────────────────────┐
                  │      emit.rs        │   SolutionSet builder; routes each binding
                  │  (Substitution +    │   through `simplify` (cfg.simplify_outputs)
                  │   SolutionSet)      │   and assembles `empty_reason` if applicable
                  └──────────┬──────────┘
                             │
                             ▼
                       SolutionSet
```

### 2.3 Module layout

```
crates/monomix-kernel/src/solve/
├── mod.rs              — public API, SolverError, SolverConfig, re-exports
├── result.rs           — SolutionSet, Substitution, EmptyReason, builder helpers
├── normalize.rs        — Eq(l,r) → l-r ; bare expr → expr ; constant detection
├── single/
│   ├── mod.rs          — driver_single: dispatch on poly::deg
│   ├── linear.rs       — single-equation linear: -b/a
│   ├── quadratic.rs    — quadratic formula + discriminant sign analysis
│   └── special.rs      — pre-quadratic recognition of x² = c, (x−r)(x−s) = 0
├── system/
│   ├── mod.rs          — driver_system: linearity check, dispatch
│   ├── matrix.rs       — augmented matrix construction from equation list
│   ├── eliminate.rs    — forward elimination with partial pivoting
│   └── back_sub.rs     — back-substitution + consistency check
└── tests.rs
```

The split mirrors the kernel convention from `simplify/` and `poly/`: a thin driver per
input shape (single vs. system), focused leaf modules per algorithm, and emission
collected in one place. The `single/` and `system/` directories are siblings rather than
nested because the system path does not invoke the single-equation path — they share
`normalize.rs` and `result.rs` but not the dispatch logic.

### 2.4 Algorithm dispatch at a glance

| Input shape | Dispatch | Algorithm | Output | Failure mode |
|-------------|----------|-----------|--------|--------------|
| `c = c` (constant equality) | `normalize → constant_check` | None — return `AllSolutions` | `{}` with `empty_reason: AllSolutions` | n/a |
| `c₁ = c₂` (constant inequality) | `normalize → constant_check` | None — return `Inconsistent` | `{}` with `empty_reason: Inconsistent` | n/a |
| `a·x + b = 0`, `a ≠ 0` | `poly::view → deg = 1` | `x = −b/a`; one substitution | `[{x: −b/a}]` | `a == 0`: degenerate, fall through to constant case |
| `a·x² + b·x + c = 0`, `a ≠ 0` | `poly::view → deg = 2` | Quadratic formula; two substitutions | `[{x: r₁}, {x: r₂}]` | numeric `D < 0` → `[]` with `NoRealRoots` |
| `(x − r₁)(x − r₂) = 0` | `poly::view → deg = 2` (after expand) | Same as quadratic; the simplifier's expand path normalizes first | Same | Same |
| `x² = c` (numeric `c ≥ 0`) | special-form recognizer in `single/special.rs` | `x = ±√c` | `[{x: √c}, {x: −√c}]` | `c < 0` numeric → `[]` with `NoRealRoots` |
| `x³ + … = 0` | `poly::view → deg = 3` | None (Phase 3+) | n/a | `UnsupportedEquationForm { DegreeTooHigh(3) }` |
| `sin(x) = 1` | `poly::view → Err(NotPolynomial)` | None | n/a | `UnsupportedEquationForm { NonPolynomial(NonPolynomialFunction(Sin)) }` |
| `n × n` linear system | per-row `poly::view`, `poly::deg ≤ 1` | Gaussian elimination + partial pivoting + back-sub | `[{x₁: …, x₂: …, …, xₙ: …}]` or `[]` | non-linear row → `UnsupportedEquationForm` |
| `n × m` non-square system | early reject in `driver_system` | None | n/a | `NonSquareSystem { n_eqs: n, n_vars: m }` |

The conspicuous absence is cubic and quartic via Cardano and Ferrari. They are
mathematically tractable but produce expressions that are pragmatically unusable without
nested radical simplification (which we do not have in Phase 1). SCOPE.md §1.6
explicitly defers them, and the simplifier (`designs/simplifier.md` §3.5) likewise has
no path that could prove `√(a + √b) = c + √d`. The `Phase 3` candidate features
(SCOPE.md §3.2 Groebner, §3.3 tensors) are the right venue to revisit.

### 2.5 Output representation — SolutionSet, not a list of `Eq`

A natural alternative is to return the solution as an `ExprId` of the form
`List(Eq(x, e₁), Eq(x, e₂))`, matching REDUCE's display convention. We deliberately
return a structured `SolutionSet` instead, with the `ExprId`-based form available via a
`to_expr` helper in `result.rs`. Three reasons:

1. **The Python API is the primary surface.** The Python `solve(...)` function returns
   a `list[dict[Symbol, Expr]]` (one `dict` per substitution) — this is the natural
   shape for the user, and the natural shape for the MCP layer (Phase 1.5) which
   serializes it directly to a JSON array of objects.
2. **The simplifier should not see solution sets.** A `List(Eq, Eq)` ExprNode would
   route through `simplify`, which has rules for `Add`/`Mul`/`Div` but no concept of a
   "solution list" — it would either no-op or, worse, recursively simplify the inner
   `Eq` LHSs into something nonsensical. Keeping solutions in a separate type sidesteps
   this.
3. **The empty-set reason needs its own channel.** SCOPE.md §1.6 mandates a warning on
   no-real-roots, and the only place that warning naturally lives is the `SolutionSet`
   itself. Encoding `NoRealRoots(discriminant=−4)` in an `ExprNode` would require a
   sentinel variant the rest of the kernel has no use for.

The `to_expr` helper exists for callers (the REPL) that want to render the solution
back as a parsed-looking expression. It is one-way: there is no `from_expr` that
reverses solution-list parsing into `SolutionSet`.

### 2.6 Single-call, no fixed-point

Unlike the simplifier (`designs/simplifier.md` §2.4), the solver has no fixed-point
loop. Each call performs:

1. **Normalize** the equation to `expr = 0` (one `pool.sub` plus eager normalization).
2. **Recognize** the polynomial shape via `poly::view` (one tree walk).
3. **Dispatch** on `poly::deg` to the linear or quadratic algorithm (constant time).
4. **Compute** the closed-form roots (constant time per root, modulo coefficient
   simplification).
5. **Simplify** each output `ExprId` once via `simplify` if `cfg.simplify_outputs`
   (the simplifier's own fixed-point loop subsumes the would-be solver loop).

The `n × n` system path is similarly bounded: one `poly::view` per row × column for
matrix construction, `O(n³)` coefficient operations during elimination, one back-sub
pass, and one `simplify` per output binding. There is no recursive call back into
`solve` or `solve_system`.

This contrasts with the simplifier's rewrite-until-fixed approach because the solver is
not chasing convergence — every algorithm has a static, known number of steps.

---

## 3. Deep Dive

### 3.1 Driver — single equation (`single/mod.rs`)

```rust
pub fn solve_single(
    pool: &mut ExprPool,
    cfg: &SolverConfig,
    eq: ExprId,
    var: ExprId,
) -> Result<SolutionSet, KernelError> {
    let var_sym = match pool.get(var) {
        ExprNode::Symbol(s) => Symbol(*s),
        _ => return Err(KernelError::Solver(SolverError::NotASymbol(var))),
    };

    // 1. Normalize: Eq(l, r) → l - r ; bare expr → expr.
    let zero_form = normalize::to_zero_form(pool, eq);

    // 2. Constant detection — short-circuit before the polynomial engine.
    if let Some(answer) = normalize::classify_constant(pool, zero_form, var_sym) {
        return Ok(answer);
    }

    // 3. Polynomial view in the unknown. Other symbols in the equation are treated
    // as parameters (constant coefficients) — `poly::view`'s definition of "constant"
    // is "does not contain `var_sym`" (`designs/polynomial-ops.md` §3.2), so
    // `solve(x + y = 1, x)` returns `Ok` with `y` collapsed into the x⁰ coefficient.
    // The Multivariate variant of `PolyError` only fires from `poly::add/mul`
    // mismatching variables, not from `view` itself — it cannot reach this site.
    let p = match poly::view(pool, zero_form, var_sym) {
        Ok(p) => p,
        Err(poly::PolyError::NotPolynomial { kind, span, .. }) => {
            return Err(KernelError::Solver(SolverError::UnsupportedEquationForm {
                reason: UnsupportedReason::NonPolynomial { kind },
                span,
            }));
        }
        Err(other) => return Err(KernelError::from(other)),
    };

    // 4. Degree dispatch.
    match p.terms.first().map(|t| t.exponent).unwrap_or(0) {
        0 => Ok(normalize::all_or_none(pool, &p, var_sym)),
        1 => single::linear::solve(pool, cfg, &p, var_sym),
        2 => {
            // Allow the special-form recognizer first; it handles `x² − c = 0` faster
            // than the general quadratic formula and avoids constructing the symbolic
            // discriminant when both roots are clean.
            if let Some(special) = single::special::try_recognize(pool, cfg, &p, var_sym)? {
                return Ok(special);
            }
            single::quadratic::solve(pool, cfg, &p, var_sym)
        }
        d => Err(KernelError::Solver(SolverError::UnsupportedEquationForm {
            reason: UnsupportedReason::DegreeTooHigh { degree: d },
            span: None,
        })),
    }
}
```

The driver is deliberately flat: every dispatch decision is made on a single number
(`poly::deg`), and every leaf algorithm is a separate module. The polynomial engine is
the single point where "is this polynomial in `var`?" is decided — the solver does not
duplicate that recognition logic.

The `poly::view` call is the most expensive step in the typical case (one tree walk
plus one coefficient bucket per term). A view cache on `Session` is a Phase 2 candidate
(`designs/polynomial-ops.md` §5 action item 20) but is not in Phase 1 scope; per-`solve`
view calls are cheap enough at the SCOPE.md §1.12 input sizes.

### 3.2 Normalize-to-zero (`normalize.rs`)

The first job is to turn whatever the user typed into a polynomial-recognizable shape.

```rust
/// Turn `Eq(l, r)` into `l − r`, leaving non-Eq inputs untouched. The pool's eager
/// normalization handles the case where `r == 0` already (returns `l` unmodified).
pub fn to_zero_form(pool: &mut ExprPool, eq: ExprId) -> ExprId {
    match pool.get(eq) {
        ExprNode::Eq(l, r) => {
            let neg_r = pool.neg(*r);
            pool.add(vec![*l, neg_r])
        }
        _ => eq,
    }
}

/// If `expr` (already in zero form) is a constant w.r.t. `var`, classify the
/// equation as `0 = 0` (AllSolutions) or `c = 0` (Inconsistent).
pub fn classify_constant(pool: &mut ExprPool, expr: ExprId, var: Symbol) -> Option<SolutionSet> {
    if poly::contains_symbol(pool, expr, var) {
        return None;
    }
    // expr does not contain var; if it simplifies to zero the equation is an
    // identity, otherwise it is contradictory.
    let simplified = simplify::simplify_eager(pool, expr);  // single-pass form
    if simplified == pool.zero {
        Some(SolutionSet {
            substitutions: SmallVec::new(),
            empty_reason: Some(EmptyReason::AllSolutions),
        })
    } else {
        Some(SolutionSet {
            substitutions: SmallVec::new(),
            empty_reason: Some(EmptyReason::Inconsistent { row: 0, residual: simplified }),
        })
    }
}
```

`simplify_eager` is a thin wrapper around `simplify::simplify` with `SimplifierConfig`
defaults and a fresh `SimplifyCache` — the constant-detection path does not need the
session-level cache because the input is a tiny sub-expression. The eager path is
critical because `Eq(2*3, 6)` after `pool.add` becomes `Add(2*3, -6)` which the pool
does not auto-fold to zero (it preserves structure); the simplifier must collapse it.

The "all solutions" classification is reported via `EmptyReason::AllSolutions` rather
than as a sentinel `*` substitution because Phase 1 has no representation for "any
real number". REDUCE's convention here is equally awkward — it returns `t` (true) for
identities — and the structured `EmptyReason` preserves enough information for the
Python boundary to render whichever convention is desired.

### 3.3 Linear single-equation (`single/linear.rs`)

```rust
pub fn solve(
    pool: &mut ExprPool,
    cfg: &SolverConfig,
    p: &poly::UnivPoly,
    var: Symbol,
) -> Result<SolutionSet, KernelError> {
    // p has degree exactly 1: terms = [(1, a), (0, b)] OR [(1, a)] (b = 0).
    debug_assert!(p.terms.first().map(|t| t.exponent) == Some(1));
    let a = p.terms[0].coefficient;
    let b = p.terms.get(1).map(|t| t.coefficient).unwrap_or(pool.zero);

    // x = -b / a. The pool's eager normalizations handle `0 / a = 0` and `-0 = 0`.
    let neg_b = pool.neg(b);
    let raw = pool.div(neg_b, a);
    let final_id = if cfg.simplify_outputs {
        let mut cache = SimplifyCache::new();
        simplify::simplify(pool, &SimplifierConfig::default(), &mut cache, raw)?
    } else {
        raw
    };

    Ok(SolutionSet {
        substitutions: smallvec![Substitution {
            bindings: smallvec![(var, final_id)],
        }],
        empty_reason: None,
    })
}
```

`a` cannot be zero in this branch — `poly::view` drops zero coefficients
(`designs/polynomial-ops.md` §3.1 invariant 2), and a `UnivPoly` with `terms[0]
.exponent == 1` necessarily has a non-zero leading coefficient. If it did, the term
would not be in the list and `poly::deg` would have returned 0, routing to the constant
branch. This is a load-bearing precondition asserted in debug builds.

The single Substitution's bindings list is a `SmallVec` of capacity 4 even though we
only ever store one entry here, because the same `Substitution` shape is used by
`solve_system` for `n` bindings. The `(Symbol, ExprId)` payload includes the unknown's
symbol so the Python boundary can build a `dict` keyed on the user's variable name.

### 3.4 Quadratic (`single/quadratic.rs`)

```rust
pub fn solve(
    pool: &mut ExprPool,
    cfg: &SolverConfig,
    p: &poly::UnivPoly,
    var: Symbol,
) -> Result<SolutionSet, KernelError> {
    debug_assert!(p.terms.first().map(|t| t.exponent) == Some(2));
    let a = p.terms[0].coefficient;
    let (b, c) = extract_lower(pool, p);

    // discriminant = b² − 4ac
    let b2 = pool.pow(b, pool.intern_smallint(2));
    let four_ac = pool.mul(vec![pool.intern_smallint(4), a, c]);
    let discriminant_raw = pool.sub(b2, four_ac);
    let discriminant = simplify::simplify_eager(pool, discriminant_raw);

    // Sign analysis on the *simplified* discriminant.
    match analyze_sign(pool, discriminant) {
        Sign::ProvablyNegative => {
            return Ok(SolutionSet {
                substitutions: SmallVec::new(),
                empty_reason: Some(EmptyReason::NoRealRoots { discriminant }),
            });
        }
        Sign::Zero => {
            // Repeated root: x = -b / (2a). One substitution, not two.
            let neg_b = pool.neg(b);
            let two_a = pool.mul(vec![pool.intern_smallint(2), a]);
            let r = pool.div(neg_b, two_a);
            return Ok(emit_one(pool, cfg, var, r)?);
        }
        Sign::ProvablyPositive | Sign::Unknown => {
            // x = (-b ± √D) / (2a). Both roots emitted; the Unknown case includes
            // symbolic discriminants like `b² − 4ac` where we can't decide sign — we
            // emit the symbolic radical and the user (or a future simplifier) can
            // refine.
            let sqrt_d = pool.pow(discriminant, pool.intern_rational(1, 2));
            let neg_b = pool.neg(b);
            let two_a = pool.mul(vec![pool.intern_smallint(2), a]);

            let plus  = pool.div(pool.add(vec![neg_b, sqrt_d]),       two_a);
            let minus = pool.div(pool.sub(neg_b, sqrt_d),             two_a);

            let plus_final  = if cfg.simplify_outputs { simplify_one(pool, plus)?  } else { plus  };
            let minus_final = if cfg.simplify_outputs { simplify_one(pool, minus)? } else { minus };

            Ok(SolutionSet {
                substitutions: smallvec![
                    Substitution { bindings: smallvec![(var, plus_final)]  },
                    Substitution { bindings: smallvec![(var, minus_final)] },
                ],
                empty_reason: None,
            })
        }
    }
}

fn extract_lower(pool: &ExprPool, p: &poly::UnivPoly) -> (ExprId, ExprId) {
    // p.terms is sorted descending. terms[0] is the x² term. b is the x¹ coefficient
    // (or zero); c is the x⁰ coefficient (or zero).
    let b = p.terms.iter().find(|t| t.exponent == 1).map(|t| t.coefficient).unwrap_or(pool.zero);
    let c = p.terms.iter().find(|t| t.exponent == 0).map(|t| t.coefficient).unwrap_or(pool.zero);
    (b, c)
}
```

**Substitution ordering convention.** The `+√D` root is always first, the `−√D` root
second. This is documented in `Substitution`'s doc comment; the golden-corpus tests
(§6.5) rely on it for stable comparison against REDUCE's output.

**Repeated-root case (`D = 0`).** REDUCE returns the repeated root twice in its
solution list (multiplicity is preserved). The Phase 1 implementation returns a *single*
substitution because the "list of roots" abstraction in §2.1 deliberately does not
carry multiplicity — `SolutionSet::substitutions.len()` is the count of *distinct* real
roots, not algebraic multiplicity. This is a documented divergence from REDUCE
(§6.5) and the rationale is that Python users expect set-like behaviour from a function
named `solve`. A `solve_with_multiplicity` API is a Phase 2 candidate (§5.4).

### 3.4.1 Discriminant sign analysis (`analyze_sign` in `single/quadratic.rs`)

```rust
enum Sign { ProvablyNegative, Zero, ProvablyPositive, Unknown }

fn analyze_sign(pool: &ExprPool, discriminant: ExprId) -> Sign {
    match pool.get(discriminant) {
        ExprNode::SmallInt(0)              => Sign::Zero,
        ExprNode::SmallInt(k) if *k > 0    => Sign::ProvablyPositive,
        ExprNode::SmallInt(k) if *k < 0    => Sign::ProvablyNegative,
        ExprNode::Rational(num, den) => {
            // den is always positive in the canonical form.
            match num.cmp(&BigInt::ZERO) {
                Ordering::Greater => Sign::ProvablyPositive,
                Ordering::Equal   => Sign::Zero,
                Ordering::Less    => Sign::ProvablyNegative,
            }
        }
        ExprNode::Float(f) => {
            if *f > 0.0       { Sign::ProvablyPositive }
            else if *f < 0.0  { Sign::ProvablyNegative }
            else if *f == 0.0 { Sign::Zero }
            else              { Sign::Unknown }  // NaN
        }
        // Symbolic discriminant: we cannot decide. The quadratic formula still
        // produces a valid ±√D form; the user gets a symbolic answer with a
        // symbolic radical.
        _ => Sign::Unknown,
    }
}
```

The function is deliberately conservative: only numeric atoms get a definite sign. Even
for expressions like `1 + 1` (which `simplify_eager` would have folded to `2` upstream)
we rely on the simplifier having already done the folding. A `Sign::Unknown` result is
not an error — it routes through the symbolic ±√D branch, which produces a valid
closed-form answer.

**Why we do not attempt symbolic sign analysis.** Detecting the sign of `b² − 4ac` for
arbitrary symbolic `a, b, c` is undecidable in general (it requires positivstellensatz
or interval arithmetic), and even simple cases (`(x − 1)² + 1 > 0` for real `x`) would
require a SAT-modulo-reals solver. The simplifier's normal-form contract
(`designs/simplifier.md` §1.1) is what does the easy cases for us: anything like
`(1)² − 4·1·0` simplifies to `1` upstream, and `analyze_sign` then reports
`ProvablyPositive`. This is the pragmatic Phase 1 stance — Phase 3+ can revisit if the
complex-numbers feature lands.

### 3.5 Special-form recognition (`single/special.rs`)

The general quadratic formula always works, but it is wasteful for `x² = c` and
`(x − r)(x − s) = 0` shapes — both are exact and should produce clean roots without
constructing a `√(0 + 4·c)` and asking the simplifier to clean up.

```rust
pub fn try_recognize(
    pool: &mut ExprPool,
    cfg: &SolverConfig,
    p: &poly::UnivPoly,
    var: Symbol,
) -> Result<Option<SolutionSet>, KernelError> {
    debug_assert!(p.terms.first().map(|t| t.exponent) == Some(2));

    // Shape 1: pure-power, x² + c = 0 with no x¹ term.
    if !p.terms.iter().any(|t| t.exponent == 1) {
        return Ok(Some(solve_pure_power(pool, cfg, p, var)?));
    }

    // Shape 2: monic with integer roots — (x − r₁)(x − r₂) form.
    // `(x − r₁)(x − r₂) = x² − (r₁+r₂)x + r₁·r₂` so we check whether c factors over Z
    // such that two integer divisors sum to −b. Cheap when c and b are SmallInt.
    if let Some(roots) = try_integer_factor(pool, p) {
        return Ok(Some(emit_two_roots(pool, cfg, var, roots)?));
    }

    // No special form recognized; the caller falls through to the general quadratic.
    Ok(None)
}
```

**`solve_pure_power` (`x² = c`).** This is the most common Phase 1 input — every
SCOPE.md §1.6 example after the linear one is of this shape.

```rust
fn solve_pure_power(
    pool: &mut ExprPool,
    cfg: &SolverConfig,
    p: &poly::UnivPoly,
    var: Symbol,
) -> Result<SolutionSet, KernelError> {
    let a = p.terms[0].coefficient;
    let c = p.terms.iter().find(|t| t.exponent == 0).map(|t| t.coefficient).unwrap_or(pool.zero);
    // a·x² + c = 0 ⟹ x² = -c/a ⟹ x = ±√(-c/a).
    let neg_c_over_a = simplify::simplify_eager(pool, pool.div(pool.neg(c), a));
    match analyze_sign(pool, neg_c_over_a) {
        Sign::ProvablyNegative => Ok(SolutionSet {
            substitutions: SmallVec::new(),
            empty_reason: Some(EmptyReason::NoRealRoots { discriminant: neg_c_over_a }),
        }),
        Sign::Zero => Ok(emit_one(pool, cfg, var, pool.zero)?),
        Sign::ProvablyPositive | Sign::Unknown => {
            let root = pool.pow(neg_c_over_a, pool.intern_rational(1, 2));
            let neg_root = pool.neg(root);
            Ok(SolutionSet {
                substitutions: smallvec![
                    Substitution { bindings: smallvec![(var, simplify_one(pool, root)?)]     },
                    Substitution { bindings: smallvec![(var, simplify_one(pool, neg_root)?)] },
                ],
                empty_reason: None,
            })
        }
    }
}
```

The `Sign::Unknown` case for symbolic `−c/a` produces `x = ±√(−c/a)`. This is *not* the
same shape as the general quadratic's `(−b ± √D) / (2a)` — it lacks the unnecessary
`(0)/(2a)` ratio. The simplifier could in principle fold the general form down to this,
but the special-form recognizer saves the round trip.

**`try_integer_factor`.** The classic integer-root recognizer for `x² + bx + c = 0`
where both `b` and `c` are `SmallInt`. Walks divisors of `c` once, tries each pair
`(r₁, r₂)` with `r₁·r₂ == c` for a sum equalling `−b`. O(d(c)) where `d(c)` is the
divisor count — bounded by `~40` for any `c < 10^9` in practice. If the search fails or
either of `b`, `c` is symbolic, returns `None` and the caller falls through.

This is a small optimization that pays for itself in the golden-corpus tests: REDUCE's
solver produces clean integer roots for textbook quadratics, and matching that output
without nested radical simplification means recognizing the case before invoking the
formula.

### 3.6 Linear systems (`system/`)

The system path is the most algorithmically substantial part of Phase 1 solving. It
applies Gaussian elimination with partial pivoting to an `n × n` augmented matrix of
coefficient `ExprId`s.

#### 3.6.1 Driver (`system/mod.rs`)

```rust
pub fn solve_system(
    pool: &mut ExprPool,
    cfg: &SolverConfig,
    eqs: &[ExprId],
    vars: &[ExprId],
) -> Result<SolutionSet, KernelError> {
    if eqs.len() != vars.len() {
        return Err(KernelError::Solver(SolverError::NonSquareSystem {
            n_eqs: eqs.len(),
            n_vars: vars.len(),
        }));
    }
    let var_syms = vars_as_symbols(pool, vars)?;       // also rejects duplicates
    let matrix = matrix::build(pool, eqs, &var_syms)?; // n × (n+1) augmented

    // Forward elimination with partial pivoting.
    let echelon = eliminate::forward(pool, cfg, matrix)?;

    // Back-substitution; may yield Inconsistent or AllSolutions in the rank-deficient
    // case (Phase 1 rejects the latter as UnsupportedEquationForm — see §3.6.4).
    back_sub::run(pool, cfg, echelon, &var_syms)
}
```

#### 3.6.2 Augmented matrix construction (`system/matrix.rs`)

For each equation `eq_i`, the matrix builder:

1. Normalizes to zero form (`normalize::to_zero_form`).
2. For each `var_j` in `vars`, calls `poly::coeff(pool, zero_form, var_j, 1)` and
   stores it in `M[i][j]`.
3. Calls `poly::view(pool, zero_form, var_j)` and asserts every term has exponent ≤ 1.
   Any exponent ≥ 2 is non-linear and rejects the system.
4. Computes the constant column: `M[i][n] = -((zero_form) - Σ_j M[i][j] * var_j)`,
   the part of the equation independent of all unknowns. Equivalently, it is the value
   of `zero_form` with every `var_j` substituted by 0 — and then negated to put it on
   the RHS of `M·x = b`.

```rust
pub fn build(
    pool: &mut ExprPool,
    eqs: &[ExprId],
    vars: &[Symbol],
) -> Result<Matrix, KernelError> {
    let n = vars.len();
    let mut m = Matrix::zeros(pool, n, n + 1);
    for (i, &eq) in eqs.iter().enumerate() {
        let zf = normalize::to_zero_form(pool, eq);
        // Linearity check via poly::view in *each* unknown.
        for &v in vars {
            let p = poly::view(pool, zf, v).map_err(|e| match e {
                poly::PolyError::NotPolynomial { kind, span, .. } =>
                    KernelError::Solver(SolverError::UnsupportedEquationForm {
                        reason: UnsupportedReason::NonPolynomial { kind },
                        span,
                    }),
                other => KernelError::from(other),
            })?;
            if p.terms.iter().any(|t| t.exponent > 1) {
                return Err(KernelError::Solver(SolverError::UnsupportedEquationForm {
                    reason: UnsupportedReason::NonLinearSystem { equation_index: i },
                    span: None,
                }));
            }
            // Linear coefficient is `coeff(zf, v, 1)`; constant contribution gets
            // accumulated into the RHS column outside this loop.
            m[i][var_index(vars, v).unwrap()] = poly::coeff(pool, zf, v, 1)?;
        }
        // Constant: substitute every var → 0 and simplify; negate for RHS.
        let const_part = simplify::simplify_eager(
            pool,
            substitute::all_to_zero(pool, zf, vars),
        );
        m[i][n] = pool.neg(const_part);
    }
    Ok(m)
}
```

`substitute::all_to_zero` is a convenience wrapper that calls
`Session.substitute(zf, var_j, pool.zero)` for each `var_j`. The substitution engine
(SCOPE.md §1.8) is a separate component; for Phase 1 it is implemented as a tree walk
that rebuilds `ExprId`s with the matching symbol replaced by zero. The simplifier
folds the result.

**Why per-`var` `poly::view` rather than a multivariate view.** Phase 1's polynomial
engine is deliberately univariate (`designs/polynomial-ops.md` §2.5). The system path
calls `view` `n` times per equation — once per unknown — and assembles the multivariate
information at the matrix level. This is `O(n)` views per equation, but each view is
linear in the equation's tree size, so the total cost is `O(n · Σ_i |eq_i|)`. For
the SCOPE.md §1, "10×10 systems with rational coefficients" benchmark target, this is
~100 `view` calls on small expressions — well within the 1-second budget.

When Phase 2's multivariate engine lands (`designs/polynomial-ops.md` §5.1), this loop
collapses to a single `multi::view(zf, vars)` call returning an `n`-dimensional
coefficient table, and the matrix builder simplifies to a transcription. The interface
of `system::matrix::build` does not change.

#### 3.6.3 Forward elimination with partial pivoting (`system/eliminate.rs`)

```rust
pub fn forward(
    pool: &mut ExprPool,
    cfg: &SolverConfig,
    mut m: Matrix,
) -> Result<Matrix, KernelError> {
    let n = m.rows();
    for k in 0..n {
        // Partial pivoting: pick the row with the largest |M[i][k]| for i ≥ k. For
        // symbolic coefficients we fall back to "first non-zero".
        let pivot_row = if cfg.partial_pivot {
            select_pivot(pool, &m, k)
        } else {
            (k..n).find(|&i| !is_definitely_zero(pool, m[i][k]))
        };
        let pivot_row = match pivot_row {
            Some(r) => r,
            None    => continue,  // Column k is all zeros below row k; rank-deficient.
        };
        if pivot_row != k {
            m.swap_rows(k, pivot_row);
        }

        let pivot = m[k][k];
        // Eliminate column k below the pivot.
        for i in (k + 1)..n {
            let leading = m[i][k];
            if is_definitely_zero(pool, leading) { continue; }
            // factor = leading / pivot ; row_i := row_i − factor · row_k
            let factor = simplify::simplify_eager(pool, pool.div(leading, pivot));
            for j in k..(n + 1) {
                let scaled = pool.mul(vec![factor, m[k][j]]);
                let updated = pool.sub(m[i][j], scaled);
                m[i][j] = simplify::simplify_eager(pool, updated);
            }
        }
    }
    Ok(m)
}

/// Pivot selection — numeric magnitudes win unconditionally; symbolic columns fall
/// back to `first non-zero` ordering.
fn select_pivot(pool: &ExprPool, m: &Matrix, k: usize) -> Option<usize> {
    let mut best: Option<(usize, BigRational)> = None;
    let mut first_nonzero_symbolic: Option<usize> = None;
    for i in k..m.rows() {
        let v = m[i][k];
        if is_definitely_zero(pool, v) { continue; }
        match try_as_rational(pool, v) {
            Some(q) => {
                let mag = q.abs();
                if best.as_ref().map_or(true, |(_, b)| mag > *b) {
                    best = Some((i, mag));
                }
            }
            None => {
                if first_nonzero_symbolic.is_none() {
                    first_nonzero_symbolic = Some(i);
                }
            }
        }
    }
    best.map(|(i, _)| i).or(first_nonzero_symbolic)
}
```

**Pivot selection rules in detail.**

1. If any row has a *numeric* (rational or integer) leading coefficient in column `k`,
   choose the row with the largest magnitude. This is standard partial pivoting and
   bounds the growth of intermediate coefficients — critical for numerical stability
   when the system is ill-conditioned, and useful even in exact arithmetic for
   keeping the simplifier's input size modest.
2. If no row in column `k` is numeric, choose the first row with a `is_definitely_zero
   == false` leading entry. This is REDUCE's behaviour for symbolic systems — it
   matches the `solve` output convention even in cases where partial pivoting would
   have chosen differently if extended to symbolic comparisons.
3. If every entry in column `k` (rows `k..n`) is zero, the column is dependent and we
   skip without permuting. The eventual back-substitution will detect rank deficiency
   and return `Inconsistent` or reject as `UnsupportedEquationForm`.

**`is_definitely_zero` is conservative.** The function returns `true` only when the
`ExprId` *is* `pool.zero` after the upstream `simplify_eager` call. A symbolic
expression like `(a + 1) − a` that has not yet simplified to `1 − 0 = 1` would be
considered "non-zero" for pivot purposes, which is the safe choice: false-zero would
lead to a divide-by-zero panic, false-non-zero merely picks a worse pivot.

```rust
fn is_definitely_zero(pool: &ExprPool, e: ExprId) -> bool {
    e == pool.zero
}
```

#### 3.6.4 Back-substitution (`system/back_sub.rs`)

```rust
pub fn run(
    pool: &mut ExprPool,
    cfg: &SolverConfig,
    m: Matrix,
    vars: &[Symbol],
) -> Result<SolutionSet, KernelError> {
    let n = vars.len();
    // Detect rank deficiency: if any row is all-zero in the variable columns but has
    // a non-zero RHS, the system is inconsistent. If the RHS is also zero, the row
    // is dependent and the system is under-determined (Phase 1 rejects).
    for i in 0..n {
        let var_cols_zero = (0..n).all(|j| is_definitely_zero(pool, m[i][j]));
        if var_cols_zero {
            if is_definitely_zero(pool, m[i][n]) {
                // Dependent row — rank < n. Phase 1 declines.
                return Err(KernelError::Solver(SolverError::UnsupportedEquationForm {
                    reason: UnsupportedReason::NonLinearSystem { equation_index: i },
                    span: None,
                }));
            } else {
                return Ok(SolutionSet {
                    substitutions: SmallVec::new(),
                    empty_reason: Some(EmptyReason::Inconsistent {
                        row: i,
                        residual: m[i][n],
                    }),
                });
            }
        }
    }

    // Back-substitute.
    let mut bindings: SmallVec<[(Symbol, ExprId); 4]> =
        SmallVec::with_capacity(n);
    let mut values: Vec<ExprId> = vec![pool.zero; n];
    for i in (0..n).rev() {
        // x_i = (M[i][n] − Σ_{j > i} M[i][j] · x_j) / M[i][i]
        let mut rhs = m[i][n];
        for j in (i + 1)..n {
            let m_ij = m[i][j];
            if !is_definitely_zero(pool, m_ij) {
                let prod = pool.mul(vec![m_ij, values[j]]);
                rhs = pool.sub(rhs, prod);
            }
        }
        let value_raw = pool.div(rhs, m[i][i]);
        let value = if cfg.simplify_outputs {
            simplify_one(pool, value_raw)?
        } else {
            value_raw
        };
        values[i] = value;
    }
    for (i, &v) in vars.iter().enumerate() {
        bindings.push((v, values[i]));
    }

    Ok(SolutionSet {
        substitutions: smallvec![Substitution { bindings }],
        empty_reason: None,
    })
}
```

**Why under-determined systems error rather than parameterize.** A rank-`r` system in
`n > r` unknowns has an `(n − r)`-dimensional family of solutions. Representing this
requires either (a) free symbols introduced by the solver (e.g. `x = y, y = arb_1`) or
(b) a return type that carries the kernel of the matrix. Both are Phase 2+
deliverables: free-symbol introduction needs a session-side gensym counter (SCOPE.md
§1.3) that is not Phase 1, and return-as-kernel changes the public API shape for an
edge case Phase 1 inputs do not exercise. Rejecting with `UnsupportedEquationForm` is
the explicit Phase 1 stance, with a structured `reason` that says *what specifically*
the user would need to do (typically: provide more equations, or reduce the unknown
list).

**Why an inconsistent system is not an error.** `solve({x = 1, x = 2}, {x})` is a
well-formed input with a real algebraic answer ("no `x` satisfies both"). Returning the
empty `SolutionSet` with `EmptyReason::Inconsistent` matches REDUCE's behaviour and is
the contract the Python boundary uses to format the user-facing message. Raising an
exception here would be wrong — the equation system *was* solved; the answer is "no
solutions exist".

### 3.7 Solution emission and `to_expr` (`result.rs`)

`SolutionSet` has a `to_expr` helper for callers (parser-driven REPL paths) that want to
render the result as a parseable expression:

```rust
impl SolutionSet {
    /// Render this SolutionSet as a `List` of `Eq(var, value)` (single equation) or
    /// `List` of `List` of `Eq(var, value)` (system, one inner list per substitution).
    /// Empty SolutionSets render as `pool.empty_set` regardless of `empty_reason`.
    pub fn to_expr(&self, pool: &mut ExprPool, var_count_hint: usize) -> ExprId {
        if self.substitutions.is_empty() {
            return pool.empty_set;
        }
        let render_one = |sub: &Substitution, pool: &mut ExprPool| -> ExprId {
            let eqs: Vec<ExprId> = sub.bindings.iter()
                .map(|(s, e)| pool.eq(pool.symbol(*s), *e))
                .collect();
            if eqs.len() == 1 { eqs[0] } else { pool.list(eqs) }
        };
        if var_count_hint == 1 && self.substitutions.iter().all(|s| s.bindings.len() == 1) {
            // Single-unknown case: flat list of `x = value` Eq nodes.
            let eqs: Vec<ExprId> = self.substitutions.iter()
                .map(|s| pool.eq(pool.symbol(s.bindings[0].0), s.bindings[0].1))
                .collect();
            pool.list(eqs)
        } else {
            // System case: list of substitutions, each rendered as inner `List` of Eq.
            let inners: Vec<ExprId> = self.substitutions.iter()
                .map(|s| render_one(s, pool))
                .collect();
            pool.list(inners)
        }
    }
}
```

The `pool.empty_set` and `pool.list` constructors are minor extensions to the
`ExprPool` API that the DAG design (`designs/expression-dag.md` §3.5) needs to add
explicitly — `List` is mentioned briefly there as a future Fn-tagged construct, and
`empty_set` is the symbolic constant `{}`. Both are listed in §7 as coordination items.

### 3.8 PyO3 boundary

The solver is exposed to Python at `solve` (single equation) and `solve` (system, same
name, dispatched by argument shape on the Python side):

```rust
#[pyfunction]
fn solve(py: Python<'_>, session: &PySession, eq: &PyExpr, var: &PyExpr)
    -> PyResult<PySolutionSet>
{
    let pool_handle = session.pool.clone();
    let cfg         = session.solver_cfg.clone();
    let id_eq       = eq.id;
    let id_var      = var.id;
    let subtree_size = pool_handle.read().subtree_size(id_eq);
    let result = if subtree_size > 500 {
        py.allow_threads(|| {
            let mut pool = pool_handle.write();
            monomix_kernel::solve(&mut pool, &cfg, id_eq, id_var)
        })?
    } else {
        let mut pool = pool_handle.write();
        monomix_kernel::solve(&mut pool, &cfg, id_eq, id_var)?
    };
    // Emit SCOPE.md §1.6 warning when the empty set is the no-real-roots variant.
    if let Some(EmptyReason::NoRealRoots { .. }) = result.empty_reason {
        py.import("warnings")?.call_method1(
            "warn",
            ("no real solutions; complex roots not supported until Phase 3",
             py.get_type::<MonomixWarning>()),
        )?;
    }
    Ok(PySolutionSet::from(pool_handle, result))
}
```

`PySolutionSet` is a small wrapper exposed to Python as `monomix.SolutionSet` with:

- `__iter__` over substitutions (each yielded as a `dict[str, Expr]`).
- `__len__` returning the number of substitutions.
- `__bool__` returning `len(self) > 0` — so `if solve(eq, x):` does the right thing
  on no-solutions.
- A `.empty_reason` attribute exposing the structured reason (one of `"no_real_roots"`,
  `"inconsistent"`, `"all_solutions"`, `None`).

**GIL release threshold.** Same `subtree_size > 500` cutoff as the simplifier
(`designs/simplifier.md` §3.8) and the polynomial engine (`designs/polynomial-ops.md`
§3.8). This is a uniform policy across all kernel-bound surface ops, which keeps the
boundary-overhead profile predictable.

**System variant.** `solve(eqs: list[Expr], vars: list[Expr])` is a separate
`#[pyfunction]` named `solve_system` internally; Python's `solve()` binds whichever
arity matches the call signature (single Expr → single, list of Expr → system). The
system path uses the same warning emission for `NoRealRoots` and the same threshold for
GIL release; `subtree_size` is computed as `Σ subtree_size(eq_i)` for the system case.

### 3.9 Error handling

| Error | Source | Handling |
|-------|--------|----------|
| `SolverError::UnsupportedEquationForm { reason: DegreeTooHigh(d) }` | `driver_single` after `poly::deg(p) ≥ 3` | Return; user sees `"equation form not supported: polynomial degree 3 (Phase 3+)"` |
| `SolverError::UnsupportedEquationForm { reason: NonPolynomial(_) }` | `poly::view` returns `NotPolynomial` | Return; the `kind` payload tells the user *which* subterm was the problem |
| `SolverError::UnsupportedEquationForm { reason: NonLinearSystem { equation_index } }` | `system::matrix::build` finds a degree-2+ term | Return; the index pinpoints the offending equation in the input list |
| `SolverError::UnsupportedEquationForm { reason: DuplicateUnknowns }` | `system::vars_as_symbols` finds a repeated symbol | Return; rejected before any matrix work |
| `SolverError::NotASymbol(id)` | `pool.get(var)` is not `Symbol` | Return; usually a parser-side bug rather than user error |
| `SolverError::NonSquareSystem` | `solve_system` early check on lengths | Return; phrased to suggest adding/removing equations |
| Pass-through `KernelError` | simplifier or polynomial engine errors | Propagate unchanged; the solver does not wrap them |

The solver never panics. The `debug_assert!` invariants in `single/linear.rs`,
`single/quadratic.rs`, and the matrix builders catch internal violations in debug
builds and become benign no-ops in release builds — the function returns its computed
output even if intermediate sanity checks would have triggered.

The `Span` field on `UnsupportedEquationForm` is populated when `poly::view` reports a
span (the parser's `SpanMap` flows through unchanged). For `DegreeTooHigh` and
`NonLinearSystem` no span is attached because the failure is a property of the whole
equation, not a specific subterm; the Python boundary can render the equation index
instead.

---

## 4. Trade-off Analysis

### 4.1 Closed-form quadratic vs. completing the square

**Chosen: standard quadratic formula `(−b ± √(b² − 4ac)) / (2a)`.**

The textbook formula is the simplest possible implementation: three coefficient
extractions, one discriminant computation, two divisions. Completing the square
(rewriting `ax² + bx + c = a(x + b/2a)² + c − b²/4a` and solving the perfect-square
form) produces the same final answer through a different chain of arithmetic.

| Approach | Allocations per quadratic | Output shape | Edge cases |
|----------|---------------------------|--------------|------------|
| Quadratic formula (chosen) | ~6 ExprIds before simplify | `(-b ± sqrt(D)) / (2a)` | Numeric `D < 0` directly observable |
| Completing the square | ~10 ExprIds before simplify | `-b/(2a) ± sqrt(b²/(4a²) − c/a)` | Same, but discriminant is buried inside a difference of fractions |

The quadratic formula wins on simplicity and on directness of the discriminant — sign
analysis (§3.4.1) inspects a single `ExprId`. Completing the square hides the
discriminant inside `b²/(4a²) − c/a` and forces a `simplify` round trip before sign
analysis is meaningful.

The trade-off would flip if Phase 2's "make_common_denominator" config (`designs/
simplifier.md` §2.1) were on by default, because the completing-the-square output
then collapses to a cleaner shape via the simplifier's own work. Phase 1 has the flag
off by default (`designs/simplifier.md` §1.4), so the formula's output is the cleaner
one.

### 4.2 `view`-then-coefficients vs. AST pattern matching

**Chosen: `poly::view` once per equation per variable, then `poly::coeff` for each
power.**

The alternative is to walk the equation's AST directly, matching shapes like
`Add(Mul(c, Pow(x, 2)), Mul(b, x), c)` and extracting coefficients structurally. This
is the approach REDUCE's `solveeq` uses
([solve.red:.../packages/solve/](../legacy/reduce-algebra-code-r7357-trunk/packages/solve/)),
because it predates the polynomial engine.

| Approach | Lines of dispatch code | Handles `(x+1)·(x+2)` | Handles symbolic coefficients |
|----------|------------------------|-----------------------|--------------------------------|
| AST pattern matching | ~200 (per shape × dispatch) | Only after upstream `expand` | Yes, but tangled with shape recognition |
| `poly::view` (chosen) | ~30 (linear + quadratic dispatchers) | Yes — `view` recognizes any polynomial shape | Yes — coefficients are `ExprId`s |

The AST pattern match wins on raw speed for canonical inputs but loses on every
non-canonical one. `poly::view` is a single tree walk that handles `(x+1)·(x+2)`, `x*x
+ 3*x + 2`, `2*x*x + 3*x + 2`, and every other shape that simplifies to `x² + 3x + 2`,
because it bucket-coalesces by exponent (`designs/polynomial-ops.md` §3.2). The solver
gets shape robustness for free.

The cost is one `view` traversal per equation per variable, which is the matrix
builder's `O(n)` factor in §3.6.2. For the SCOPE.md §1.6 input sizes this is negligible.

### 4.3 Partial pivoting vs. complete pivoting vs. no pivoting

**Chosen: partial pivoting (column pivot, by row), with symbolic fallback.**

| Approach | Stability for rational coeffs | Stability for symbolic | Implementation cost |
|----------|-------------------------------|------------------------|---------------------|
| No pivoting | Fails on `[[0, 1], [1, 0]] · x = [1, 0]` | Same | None — direct elimination |
| Partial pivoting (chosen) | Bounds intermediate coeff growth | Falls back to "first non-zero" | One row swap per outer iteration |
| Complete pivoting | Stronger bound on growth | No useful symbolic generalization | Column swap + permutation tracking |

Partial pivoting is the textbook default and gives the right behaviour on
near-singular numeric systems while degrading gracefully to "first non-zero" on
symbolic systems. Complete pivoting (also pivoting columns) is overkill for Phase 1
sizes and complicates the variable mapping — the back-substitution would have to
reverse the column permutation. The implementation cost is not justified by SCOPE.md
§1.6's input sizes.

The symbolic fallback rule ("first non-zero in the column") matches REDUCE's behaviour
([solve.red:.../packages/solve/solve1.red](../legacy/reduce-algebra-code-r7357-trunk/packages/solve/))
and is what the golden corpus expects.

### 4.4 Substitution-based vs. symbolic linear-system path

**Chosen: matrix-based Gaussian elimination, even for `n = 2` and `n = 3`.**

A natural shortcut is to special-case `n = 2` (Cramer's rule) and `n = 3` (Cramer or
hand-unrolled elimination) and only fall back to general Gaussian elimination for
`n ≥ 4`. REDUCE does this in places.

| Approach | Code paths | Output shape consistency | Performance at small `n` |
|----------|------------|--------------------------|--------------------------|
| Cramer for `n ∈ {2, 3}` + Gaussian otherwise | 3 separate algorithms | Different per `n` | Marginally faster for `n = 2, 3` |
| Gaussian for all `n` (chosen) | One algorithm | Identical for all `n` | Negligibly slower at `n = 2, 3` |

Single-algorithm wins on testability — the property tests in §6.2 cover all `n`
uniformly, and the golden corpus comparisons against REDUCE only need one expected
output shape. Cramer's rule on `n = 3` produces a determinant-of-3×3 expression that is
either expanded (giving the same form as Gaussian) or left as `det(...)` (which the
simplifier has no rule for in Phase 1 — `det` is a Phase 2 deliverable per SCOPE.md
§2.3).

The performance trade is decisively in Gaussian's favour: the `O(n³)` overhead at `n =
3` is 27 coefficient ops vs. ~15 for Cramer, both well under a millisecond, both
dominated by the per-coefficient `simplify` calls.

### 4.5 Eager vs. deferred output simplification

**Chosen: eager (`cfg.simplify_outputs` defaults to `true`).**

The solver could emit raw quadratic-formula output and rely on the *user* to call
`simplify` on each substitution after the fact. This would let the user choose whether
to pay the simplification cost.

| Approach | Default UX | Test predictability | Performance on chained calls |
|----------|------------|---------------------|------------------------------|
| Eager simplify (chosen) | Clean output by default | Each test asserts a single canonical shape | Slight overhead — typically <10% of total `solve` time |
| Deferred simplify | User must `simplify` to read output | Tests must allow for un-simplified shape | None |

Deferred simplification is bad UX for a CAS surface function — the user calling
`solve(x² − 4 = 0, x)` expects `[2, −2]` and not `[(0 + sqrt(16))/2, (0 − sqrt(16))/2]`.
The `simplify_outputs` config knob exists for the test harness (which wants to inspect
the raw algebraic shape before `simplify` rewrites it) and for advanced callers who
plan to compose `solve` output into a larger expression that they will `simplify` at
the end anyway.

### 4.6 Architectural divergence from REDUCE — `SolutionSet` vs. list-of-Eq output

**Chosen: structured `SolutionSet` Rust type with `to_expr` for callers wanting the
list-of-Eq encoding.**

REDUCE's `solve` returns `{x = root1, x = root2}` directly as an algebraic list
([solve.red:.../packages/solve/solve.red](../legacy/reduce-algebra-code-r7357-trunk/packages/solve/)).
This is a kernel value the user can manipulate further with REDUCE's algebraic ops.

| Property | REDUCE list-of-Eq | Monomix `SolutionSet` (chosen) |
|----------|-------------------|--------------------------------|
| In-kernel manipulability | Yes — same as any algebraic value | No — separate Rust type |
| Native Python interop | Awkward — must walk the list and unpack `Eq(x, val)` per item | Direct — `for sub in solutions: print(sub[x])` |
| MCP-server JSON shape (Phase 1.5) | Requires custom serializer | Native — list of dicts |
| Empty-set warning channel | Sentinel value indistinguishable from `{}` literal | Structured `empty_reason` field |
| Multiplicity | Carried (list contains repeated entries) | Not carried (set semantics) |

The Python boundary is the primary surface (SCOPE.md §0.1 — Python wins on install UX,
AI ecosystem, plugin authoring), and the `SolutionSet` type is the natural shape for
that. The `to_expr` helper is the escape hatch for kernel-side callers; it is not on
any hot path.

The multiplicity divergence is documented in §6.5 and §3.4 — Phase 1 deliberately drops
algebraic multiplicity from quadratics (`(x − 1)² = 0` returns `[1]`, not `[1, 1]`).
A `solve_with_multiplicity` API is a Phase 2 candidate.

---

## 5. Scale, Limits, and Future Work

### 5.1 Phase 2: Cubic and quartic via Cardano and Ferrari (SCOPE.md §3.2 candidate)

Cardano's formula for cubics and Ferrari's for quartics produce closed-form roots
involving nested radicals. Phase 1 declines them because the simplifier
(`designs/simplifier.md` §3.4) cannot reduce nested radical expressions like
`∛(c + √d)` to the elementary form a user expects, and the unsimplified output is
strictly worse than no output at all (a wall of unreadable fractions).

The Phase 2 path is:

1. Add a "nested radical simplifier" rule set to `simplify::rules.rs` (§5.1 of
   `designs/simplifier.md`) covering the textbook denesting identities.
2. Add `single::cubic.rs` and `single::quartic.rs` modules following the same shape as
   `single::quadratic.rs`. Sign analysis on the cubic discriminant (§3.4.1) extends to
   distinguish the three real roots case from the one-real-two-complex case.
3. Extend the `try_recognize` special-form path to catch `x³ − c = 0` (cube roots),
   `x⁴ − c = 0` (fourth roots), and `(x − r₁)(x − r₂)(x − r₃)(x − r₄) = 0`
   (fully-factored).

The estimated effort is ~3 weeks: 1 week for cubic, 1 for quartic, 1 for the denesting
simplifier rules. Quintic and higher are mathematically impossible by Galois (no
general radical solution) and remain out of scope forever — Phase 2 will introduce
numeric root-finding for them via `nsolve` (§5.5).

### 5.2 Phase 2: General linear systems (SCOPE.md §2.3 candidate)

The Phase 1 `n × n` restriction is a deliberate cut. Phase 2's `solve_system` will
support:

- Under-determined systems (`m < n`): introduce free symbols `arb_1, arb_2, …` and
  return the family of solutions parameterized by them.
- Over-determined systems (`m > n`): least-squares fit via `Aᵀ A x = Aᵀ b`, returning
  the closed-form residual.
- Rank-deficient systems: detect rank `r < n`, reduce to row-echelon, and parameterize
  the kernel.

These need a `Session`-side gensym counter (SCOPE.md §1.3) and a slightly richer
`Substitution` shape allowing the binding RHS to reference newly introduced symbols.
The matrix path (§3.6.2 — §3.6.4) becomes the thin layer; rank detection and
parameterization is the new logic.

The interface impact is additive: `SolutionSet` gains an optional `free_parameters:
Vec<Symbol>` field, `Substitution::bindings` allows references to those parameters,
and the Python boundary marshals them as `monomix.Arbitrary` symbols.

### 5.3 Phase 2: Non-linear systems (SCOPE.md §2.6 candidate)

Truly non-linear systems (`x² + y² = 1, x + y = 0`) require Gröbner basis reduction
or resultant elimination. Both are SCOPE.md §3.2 deliverables and depend on the
Phase 2 multivariate engine (`designs/polynomial-ops.md` §5.1). The solver path then
becomes: `multi::view(eqs, vars)` → Gröbner reduction → univariate solve per chained
equation → back-substitute. The interface stays the same (`solve_system(eqs, vars)
→ SolutionSet`); the engine swaps wholesale.

### 5.4 `solve_with_multiplicity` API (Phase 2 candidate)

Some callers want algebraic multiplicity preserved. The Phase 1 `solve` deliberately
collapses repeated roots (§3.4) for set semantics. A Phase 2 `solve_with_multiplicity`
returns `Vec<(Substitution, u32)>` where the `u32` is the multiplicity. Implementation
is a single change in the quadratic and (Phase 2) cubic/quartic emitters: emit the
substitution `multiplicity` times instead of once, attach the count, drop the
de-duplication step.

### 5.5 Numeric root-finding (Phase 2+ candidate)

Phase 1 has no `nsolve` — every Phase 1 root is closed-form symbolic. Phase 2
introduces `nsolve(eq, x, x0)` for Newton iteration on a starting guess, and
`nroots(p)` for all real roots of a polynomial of any degree (Sturm sequences for
isolation, then interval bisection for refinement). Both are independent of the
symbolic solver and live in a new `solve::numeric/` sub-module; the symbolic solver
calls `nroots` only as a fallback for cubic/quartic when nested-radical simplification
fails to produce a usable elementary form.

### 5.6 View cache on `Session` (Phase 2 candidate)

The solver calls `poly::view` once per `solve` and `n²` times per `solve_system`.
Phase 2's view cache (`designs/polynomial-ops.md` §5 action item 20) would memoize
these results on `Session`, reducing the system path's `view` calls from `O(n²)` to
`O(n)` for repeated `solve_system` calls in the same session (e.g. an optimization
loop that re-solves a perturbed system). Phase 1 input sizes don't justify the
implementation cost.

### 5.7 Performance characteristics

For Phase 1, the solver's expected complexity:

| Input | Time | Allocations |
|-------|------|-------------|
| Linear single eq | O(view) + 1 simplify | ~5 ExprIds + 1 SolutionSet |
| Quadratic single eq | O(view) + 3 simplify | ~12 ExprIds + 1 SolutionSet |
| `n × n` linear system | O(n² · view) + O(n³) coeff ops + O(n) simplify | O(n²) intermediate ExprIds |
| Constant identity | O(view) + 1 simplify (the constant check) | ~2 ExprIds |

The dominant term in the system path is the `O(n³)` coefficient operations during
elimination, each of which itself triggers a `simplify_eager` to keep intermediate
expressions bounded. For SCOPE.md §1's `10 × 10 systems with rational coefficients`
target (1-second budget), the rough math is `10³ · ~1ms simplify ≈ 1s` — right at the
edge. The §6.3 benchmarks pin this and the `simplify_eager` overhead is the regression
target.

---

## 6. Testing Strategy

### 6.1 Unit tests (`cargo test`)

**Single-equation linear:**

- `solve(x = 0, x)` ⟹ `[{x: 0}]`.
- `solve(2*x + 4 = 0, x)` ⟹ `[{x: -2}]`.
- `solve((y+1)*x + (y-1) = 0, x)` ⟹ `[{x: -(y-1)/(y+1)}]` (symbolic coefficient).
- `solve(x = 5, x)` ⟹ `[{x: 5}]` (Eq with non-zero RHS).
- `solve(0*x + 3 = 0, x)` is a constant-detection pass: `[]` with `Inconsistent`.
- `solve(0*x + 0 = 0, x)` is a constant-detection pass: `[]` with `AllSolutions`.

**Single-equation quadratic — numeric coefficients:**

- `solve(x^2 - 4 = 0, x)` ⟹ `[{x: 2}, {x: -2}]` (special-form recognizer; clean roots).
- `solve(x^2 + 1 = 0, x)` ⟹ `[]` with `NoRealRoots(discriminant: -4)`.
- `solve(x^2 - 2 = 0, x)` ⟹ `[{x: sqrt(2)}, {x: -sqrt(2)}]`.
- `solve(x^2 - 2*x - 3 = 0, x)` ⟹ `[{x: 3}, {x: -1}]` (integer-factor recognizer).
- `solve(x^2 + 2*x + 1 = 0, x)` ⟹ `[{x: -1}]` (repeated root, single sub).
- `solve(x^2 + x + 1 = 0, x)` ⟹ `[]` with `NoRealRoots(discriminant: -3)`.
- `solve(2*x^2 - 8 = 0, x)` ⟹ `[{x: 2}, {x: -2}]` (after dividing by leading coeff).

**Single-equation quadratic — symbolic coefficients:**

- `solve(a*x^2 + b*x + c = 0, x)` ⟹ `[{x: (-b + sqrt(b^2 - 4*a*c))/(2*a)},
  {x: (-b - sqrt(b^2 - 4*a*c))/(2*a)}]`.
- `solve(x^2 - a = 0, x)` ⟹ `[{x: sqrt(a)}, {x: -sqrt(a)}]` (special-form, symbolic).
- `solve((x-r)*(x-s) = 0, x)` after `expand` upstream ⟹ `[{x: r}, {x: s}]` (the
  integer-factor recognizer recognizes the expanded form on numeric `r`, `s`; on
  symbolic `r`, `s` the general formula returns the same answer modulo simplification).

**Unsupported single-equation forms:**

- `solve(x^3 = 8, x)` ⟹ `Err(UnsupportedEquationForm { reason: DegreeTooHigh(3) })`.
- `solve(sin(x) = 1, x)` ⟹ `Err(UnsupportedEquationForm { reason:
  NonPolynomial(NonPolynomialFunction(Sin)) })`.
- `solve(1/x = 0, x)` ⟹ `Err(UnsupportedEquationForm { reason:
  NonPolynomial(InDenominator) })`.
- `solve(x^x = 1, x)` ⟹ `Err(UnsupportedEquationForm { reason:
  NonPolynomial(SelfReferential) })`.
- `solve(x = 0, 5)` ⟹ `Err(NotASymbol(5))`.

**Linear systems — 2 × 2:**

- `solve_system([x + y = 3, x - y = 1], [x, y])` ⟹ `[{x: 2, y: 1}]`.
- `solve_system([2*x + y = 5, x - y = 1], [x, y])` ⟹ `[{x: 2, y: 1}]`
  (partial pivoting rearranges).
- `solve_system([x + y = 1, x + y = 2], [x, y])` ⟹ `[]` with `Inconsistent`.
- `solve_system([x + y = 1, 2*x + 2*y = 2], [x, y])` ⟹ `Err(UnsupportedEquationForm
  { reason: NonLinearSystem { equation_index: 1 } })` (rank-deficient → declined).

**Linear systems — n × n with n = 3:**

- `solve_system([x + y + z = 6, x - y + z = 2, x + y - z = 0], [x, y, z])` ⟹
  `[{x: 1, y: 2, z: 3}]`.
- `solve_system([x + 2*y + 3*z = 14, 2*x + y + z = 7, x + y + z = 6], [x, y, z])` ⟹
  the unique solution; check via back-substitution.

**Linear systems — symbolic coefficients:**

- `solve_system([a*x + b*y = c, d*x + e*y = f], [x, y])` ⟹
  `[{x: (c*e - b*f)/(a*e - b*d), y: (a*f - c*d)/(a*e - b*d)}]` (Cramer-equivalent).

**Unsupported systems:**

- `solve_system([x^2 + y = 1, x + y = 0], [x, y])` ⟹ `Err(UnsupportedEquationForm
  { reason: NonLinearSystem { equation_index: 0 } })`.
- `solve_system([x + y = 1], [x, y])` ⟹ `Err(NonSquareSystem { n_eqs: 1, n_vars: 2 })`.
- `solve_system([x + y = 1, x + y = 1, x + y = 1], [x, y, z])` is syntactically `3 × 3`
  and reaches the matrix builder; the `z` column is all-zero and the resulting matrix
  is rank-deficient, so back-substitution returns `Err(UnsupportedEquationForm
  { reason: NonLinearSystem { equation_index: <last dependent row> } })` per §3.6.4.

**Systems with parameter symbols (not in `vars`):**

- `solve_system([x + y = 1, y + z = 1], [x, y])` is `2 × 2` in the declared unknowns;
  `z` is a parameter (not in `vars`). The matrix builder treats `z` as a coefficient
  (per `poly::view`'s definition of constant — see §3.1) and the system reduces to
  `[{x: z, y: 1 - z}]`. Test that the parameter passes through unchanged into the
  output bindings.

**Idempotence regression:**

- For every test case above, `solve(eq, x).to_expr(pool)` parsed back through `parse`
  and re-`solved` produces a structurally identical `SolutionSet`.
- `simplify(s)` for `s` ∈ `solve(eq, x).substitutions[0].bindings[0].1` is a no-op when
  `cfg.simplify_outputs == true` — the output is already in normal form.

### 6.2 Property-based tests (`proptest`)

- **Round-trip on linear** (the load-bearing test for the linear path): for randomly
  generated `(a, b)` with `a ≠ 0`, `simplify(a * x + b)` evaluated at `x = -b/a` is
  zero. Checked by symbolic substitution + simplify, exact equality.
- **Round-trip on quadratic** (the load-bearing test for the quadratic path): for
  randomly generated `(a, b, c)` with `a ≠ 0` and discriminant ≥ 0, both roots from
  `solve(a*x^2 + b*x + c = 0, x)` satisfy the equation. Checked by simplifying
  `a*r² + b*r + c` for each `r` in the result and asserting equality with zero.
- **No-real-roots agreement:** for randomly generated `(a, b, c)` with discriminant
  numerically negative, `solve` returns the empty SolutionSet with `NoRealRoots`. The
  generator is biased toward small integer coefficients so the discriminant computation
  itself doesn't dominate the test budget.
- **Linear-system Cramer agreement:** for randomly generated `2 × 2` and `3 × 3`
  systems with non-zero determinant, `solve_system`'s output agrees with the textbook
  Cramer formula evaluated through the simplifier. Comparison is via numerical
  evaluation at random rational points (since structural equality of two equivalent
  closed-form expressions is hard to check directly).
- **System back-substitution invariant:** for randomly generated `n × n` systems with
  unique solutions, substituting the result back into the original equations through
  `Session::substitute` and simplifying yields zero on every row.
- **Inconsistent system detection:** for randomly generated systems constructed as
  `[eq, eq with RHS perturbed]` (forced inconsistency), the solver returns
  `Inconsistent` rather than spurious solutions.
- **Determinism:** every test case in §6.1, run 1000 times with shuffled internal
  hashing seeds, produces byte-identical `SolutionSet`s.

### 6.3 Benchmarks (`criterion`)

| Benchmark | Target |
|-----------|--------|
| `solve(2*x + 4 = 0, x)` (linear, numeric) | <1 ms (the trivial path; regression guard for boundary overhead) |
| `solve(x^2 - 4 = 0, x)` (quadratic, special form) | <2 ms |
| `solve(x^2 + b*x + c = 0, x)` (quadratic, symbolic coefficients) | <5 ms |
| `solve_system` on `5 × 5` rational system | <50 ms |
| `solve_system` on `10 × 10` rational system | <500 ms (SCOPE.md §1.6 implicit target) |
| `solve_system` on `10 × 10` symbolic system | <5 s (the symbolic-coefficient case; simplify is the bottleneck) |
| `solve_system` on `20 × 20` rational system | <30 s (Phase 2 trigger threshold) |
| Constant-equation classification (`solve(0*x = 0, x)`) | <100 µs (the short-circuit) |

The "symbolic 10×10" benchmark is the regression target for the `simplify_eager` calls
inside `eliminate.rs`. If it regresses past 5s, the elimination path is creating
unsimplified intermediates that are blowing up the per-cell cost.

### 6.4 Fuzz testing (`cargo-fuzz`)

- **Target:** `solve(parse(arbitrary_bytes), x)`. Asserts (a) no panics, (b) every
  successful return either has a non-empty `substitutions` list or a `Some(empty_reason)`
  field set, never both empty without a reason.
- **Target:** `solve_system(parse_list(a), parse_vars(b))` with random byte streams.
  Asserts (a) no panics, (b) `NonSquareSystem` is returned when the parsed lengths
  disagree, (c) `UnsupportedEquationForm` is returned when any equation parses to a
  non-linear form.
- **Target:** quadratic round-trip — generate `(a, b, c)` triples directly (avoiding
  the parser for this target), call `solve`, simplify each root through `simplify`,
  evaluate `a·r² + b·r + c` and assert it simplifies to zero. Catches any algorithmic
  bug in the closed-form derivation.
- **Seed corpus:** the legacy `.tst` files
  (`legacy/reduce-algebra-code-r7357-trunk/packages/solve/*.tst`, curated to the subset
  that parses cleanly under the Phase 1 grammar) plus a hand-curated set of
  pathological inputs (deeply-nested `(x+1)·(x+1)·…·(x+1)`, very large numeric
  coefficients, equations with leading zero coefficients).
- **Run duration:** ≥1 hour per release (combined with the parser, simplifier, and
  polynomial fuzz targets).

### 6.5 Golden-corpus tests (`pytest`)

A subset of `legacy/reduce-algebra-code-r7357-trunk/packages/solve/*.{tst,rlg}` (the
ones exercising single-equation linear, single-equation quadratic, and small-`n`
systems). For each `.tst` input, parse, run `solve` or `solve_system`, render via
`SolutionSet::to_expr`, and compare against the `.rlg` output.

**Known intentional divergences from REDUCE** (recorded in the manifest with
`# reason: ...` annotations, not treated as failures):

- **No multiplicity in repeated roots.** REDUCE returns `(x + 1)^2 = 0` ⟹
  `{x = -1, x = -1}`. Monomix returns `[{x: -1}]`. Reason: set semantics chosen for
  Python ergonomics (§3.4). A `solve_with_multiplicity` API is the Phase 2 escape
  hatch (§5.4).
- **Empty set on no-real-roots, not complex roots.** REDUCE returns `{x = i, x = -i}`
  for `x^2 + 1 = 0`. Monomix returns `[]` with `MonomixWarning`. Reason: SCOPE.md §1.6
  explicitly defers complex roots to Phase 3+. The Phase 3 warning text is removed
  when the complex-numbers feature lands.
- **Symbolic discriminant produces ±sqrt form, not REDUCE's "indeterminate" sign
  message.** REDUCE issues a "could not determine sign" message in some cases; Monomix
  emits the symbolic ±sqrt and lets the user simplify or assign. Reason: the symbolic
  output is the more useful default (§3.4.1).
- **Under-determined and over-determined system rejection vs. REDUCE's parametric
  output.** REDUCE returns parameterized solutions for `solve({x + y = 1}, {x, y})`
  using its internal `arbcomplex` symbols. Monomix Phase 1 declines with
  `UnsupportedEquationForm`; Phase 2 (§5.2) introduces the parametric form.
- **Output ordering for systems.** REDUCE orders bindings alphabetically by variable
  name; Monomix preserves the user's `vars` argument order. Documented per case.

The curated set lives in `tests/golden/solve/` with the manifest mapping input file to
expected output and the `# reason: ...` annotation per case.

---

## 7. Action Items

### Phase 1 — Core implementation

1. [ ] Create `crates/monomix-kernel/src/solve/mod.rs` exposing the public API (§2.1);
       wire `SolverError` into `KernelError` with `UnsupportedEquationForm`,
       `NotASymbol`, `NonSquareSystem` variants
2. [ ] Define `SolutionSet`, `Substitution`, `EmptyReason`, `SolverConfig` in
       `result.rs` and `mod.rs` (§2.1)
3. [ ] Implement `normalize.rs` — `to_zero_form`, `classify_constant`, with
       integration through `simplify::simplify_eager` (§3.2)
4. [ ] Implement `single/linear.rs` — extract `(a, b)` from `UnivPoly`, emit `-b/a`
       through pool constructors (§3.3)
5. [ ] Implement `single/quadratic.rs` — discriminant, `analyze_sign`, ±√D emission,
       repeated-root case (§3.4, §3.4.1)
6. [ ] Implement `single/special.rs` — `solve_pure_power` for `x² = c`,
       `try_integer_factor` for `(x − r)(x − s) = 0` (§3.5)
7. [ ] Implement `single/mod.rs` driver — degree dispatch, polynomial-engine error
       mapping (§3.1)
8. [ ] Implement `system/matrix.rs` — augmented matrix construction with per-`var`
       `poly::view` linearity check (§3.6.2)
9. [ ] Implement `system/eliminate.rs` — forward elimination with partial pivoting,
       symbolic fallback (§3.6.3)
10. [ ] Implement `system/back_sub.rs` — back-substitution, rank deficiency detection,
        Inconsistent vs. UnsupportedEquationForm split (§3.6.4)
11. [ ] Implement `system/mod.rs` driver — square-shape check, dispatch (§3.6.1)
12. [ ] Add `to_expr` helper in `result.rs` for callers wanting the list-of-Eq encoding
        (§3.7)
13. [ ] Wire `solve` and `solve_system` into the Python `Session` via PyO3 with the
        same `subtree_size > 500` GIL-release threshold as the simplifier; emit the
        SCOPE.md §1.6 `MonomixWarning` on `NoRealRoots` (§3.8)
14. [ ] Coordinate with `designs/expression-dag.md` §3.5 to add `pool.list(...)`,
        `pool.empty_set`, and `pool.eq(...)` constructors used by `to_expr`
15. [ ] Coordinate with `designs/parser.md` §3.3 to bind `solve` and `solve_system`
        as parser builtins with the correct argument arities

### Phase 1 — Verification

16. [ ] Unit-test all transformations enumerated in §6.1, including the structured-
        error paths and the symbolic-coefficient cases
17. [ ] `proptest` linear and quadratic round-trip + system back-substitution
        invariant + inconsistent-detection + determinism (§6.2)
18. [ ] `criterion` benchmarks including the `10 × 10 symbolic` regression guard
        (§6.3)
19. [ ] `cargo-fuzz` target with `solve` and `solve_system`, plus quadratic-round-trip
        property check (§6.4)
20. [ ] Curate the golden-corpus `.tst`/`.rlg` subset for solve operations, with a
        divergence manifest covering the intentional divergences in §6.5
21. [ ] Confirm SCOPE.md §1.6 invariants hold: linear and quadratic give correct
        roots, no-real-roots emits warning, `n × n` systems solved by elimination

### Phase 2 — Generalization (deferred)

22. [ ] Implement `single/cubic.rs` and `single/quartic.rs` via Cardano and Ferrari
        once the simplifier has nested-radical denesting (§5.1)
23. [ ] Implement under-determined and over-determined system support with free-symbol
        introduction (`Session` gensym, §5.2)
24. [ ] Implement non-linear system solving via Gröbner basis on top of the multivariate
        polynomial engine (§5.3)
25. [ ] Add `solve_with_multiplicity` API that preserves algebraic multiplicity (§5.4)
26. [ ] Implement `nsolve` (Newton iteration) and `nroots` (Sturm + bisection) in a new
        `solve/numeric/` sub-module (§5.5)
27. [ ] Add a `view` cache on `Session` once `ExprId` is content-addressed (§5.6); the
        `solve_system` path becomes the primary beneficiary
