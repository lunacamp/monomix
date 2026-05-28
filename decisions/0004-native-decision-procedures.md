# ADR 0004 — Native decision procedures in the kernel

- Status: Accepted
- Date: 2026-05-27
- Deciders: Roman (solo maintainer)
- Supersedes: [ADR-0003](0003-z3-as-smt-backend.md) (Z3 as the SMT backend)
- Depends on: ADR-0001 (implementation language), ADR-0002 (high-level architecture)

## Context

[ADR-0003](0003-z3-as-smt-backend.md) adopted Microsoft Z3 as an external
SMT backend, reached through a `monomix.solver` facade, with a Phase-2
`rust/solver-bridge` crate linking `z3-sys`. The `feat/python-bindings`
branch then drifted further from even that: it built a Python-side bridge
(`monomix.smt`) that was *protocol-only* — a `Backend` `Protocol` plus a
`Translator`, with **no solver shipped** and the integrator expected to
supply their own adapter.

That is not the intent. Monomix should **itself** provide SMT-style
reasoning over its own theories — satisfiability, `prove`, `decide`,
`assume`, and simplification under assumptions — as a capability of the
kernel, not as a translation layer to a third-party solver and not as a
bring-your-own-backend protocol. There must be no external-solver bridge.

The subproblems a CAS actually needs decided are the ones ADR-0003 itself
enumerated: checking that an assumption set is consistent, reasoning about
the sign or range of a subexpression, eliminating impossible branches in a
piecewise definition, verifying a denominator is nonzero, and deciding
(non)linear inequality systems.

The kernel already owns building blocks for the decidable core:

- exact-rational Gaussian elimination (`solve_system` /
  `try_solve_system_exact`);
- an exact discriminant classifier (`classify_discriminant`) that reads
  sign directly from numeric atoms;
- a univariate polynomial layer (`UnivPoly`, `poly_div`, `poly_gcd`).

## Decision

Monomix implements decision procedures **natively, in the Rust kernel**,
over its own expression representation. No external SMT solver is linked,
shipped, or required; there is no backend protocol for users to implement.

The capability is exposed through the kernel's own API (`prove` / `decide`
/ `assume` / simplify-under-assumptions), reusing the result vocabulary
that proved useful in the bridge work: `Proved` / `Refuted(counterexample)`
/ `Sat(model)` / `Unsat` / `Unknown`, with **`Unknown` a first-class
return value** so the algebraic engine can fall back to symbolic methods
rather than fail. Per-symbol sorts (`real` / `int` / `bool`) come from the
existing `Session` sort metadata (`declare` / `sort_of`).

Scope is tiered by tractability; the precise phasing is set by the
forthcoming design (`designs/decision-procedures.md`):

1. **Linear arithmetic over ℚ (QF_LRA)** — exact satisfiability of linear
   equality/inequality systems (simplex or Fourier–Motzkin over
   `BigRational`), extending the existing exact Gaussian elimination from
   equalities to inequalities. Serves the assumption store and piecewise
   simplifier directly.
2. **Univariate nonlinear sign reasoning** — root isolation / Sturm
   sequences over the existing polynomial layer; decides "`p(x) > 0` on
   ℝ" and nonzero-denominator questions.
3. **Linear integer arithmetic (QF_LIA)** — adds integrality
   (branch-and-bound / the Omega test), keyed off the symbol sort metadata.
4. **Multivariate nonlinear real (QF_NRA)** — CAD / nlsat. Treated as a
   documented frontier, **not** a near-term promise; monomix may return
   `Unknown` here indefinitely.

## Alternatives considered

- **Keep ADR-0003 (embed Z3).** Rejected: a ~10 MB native C dependency
  with FFI and a build-matrix cost, and it makes monomix's reasoning a thin
  wrapper over a third party rather than a capability it owns. The
  maintainer explicitly wants the capability native.
- **Bring-your-own-backend protocol (what the branch built).** Rejected:
  it pushes the actual reasoning onto integrators — monomix would ship a
  translator and *no* decision capability of its own. That is the opposite
  of the goal.
- **Full general SMT / quantifier elimination from day one.** Rejected as
  unrealistic solo. But, unlike ADR-0003's blanket "out of scope for a solo
  project," we scope the *decidable core* (tiers 1–2) as achievable and
  grow incrementally; only the multivariate-nonlinear frontier (tier 4) is
  left open-ended.

## Consequences

Positive:

- monomix's reasoning is self-contained: no native dependency, no FFI, no
  external process — pure Rust, honoring the workspace
  `unsafe_code = "forbid"` lint.
- The decidable subproblems the CAS actually hits are served by code that
  already half-exists in the kernel.
- Exact-by-default arithmetic extends naturally to exact decision
  procedures (`BigRational`), consistent with SCOPE §1.1.

Negative / risks:

- We own the hard cases. Multivariate nonlinear decidability is genuinely
  difficult; the honest answer there is `Unknown`, and callers must handle
  it (they already do — `Unknown` is first-class).
- More kernel surface to build, test, and fuzz. The no-panic invariant
  extends to the new procedures.
- Performance on adversarial inputs is now our problem; every decision call
  needs a resource bound that returns `Unknown` rather than looping.

## Open questions

- API placement: a new kernel module (e.g. `decide/`) versus folding into
  `solve/` and a simplify-under-assumptions path. The design decides.
- How assumptions are represented and scoped (a `push`/`pop` analog)
  without a stateful solver context — likely an immutable assumption set
  threaded through calls.
- Whether counterexample/model values are returned as kernel `Expr`s from
  the start. ADR-0003's bridge deferred this; the native path has the pool
  in hand, so it is cheaper to do here.

## References

- [ADR-0003](0003-z3-as-smt-backend.md) (superseded).
- `SCOPE.md` — scope and phasing.
- `designs/decision-procedures.md` — forthcoming native design.
