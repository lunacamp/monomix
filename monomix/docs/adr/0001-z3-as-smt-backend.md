# ADR 0001 — Z3 as the SMT backend for Monomix

- Status: Accepted
- Date: 2026-05-03
- Deciders: Roman (solo maintainer)

## Context

Monomix is a modern rewrite of REDUCE. The kernel is split between Python
(driver, parser, high-level rewrite system, REPL) and Rust (term
representation, normal forms, polynomial arithmetic in the hot path).

A CAS routinely runs into subproblems that are *decidable* but that the
algebra layer reimplements badly:

- Deciding a nonlinear real inequality system (e.g. `x^2 + y^2 < 1 ∧ x+y > 1`).
- Checking that an assumed predicate is consistent before adding it to
  the assumption store.
- Simplifying expressions under sign / range assumptions on variables.
- Eliminating impossible branches in piecewise definitions.
- Verifying that a denominator is nonzero in a candidate identity.

Legacy REDUCE handles a subset of these via `redlog`, which is excellent
for real quantifier elimination but is itself a large body of historical
RLISP code we are trying to retire.

We need a backend that:

1. Decides quantifier-free nonlinear real arithmetic well.
2. Handles linear integer arithmetic, bit-vectors, arrays, datatypes, and
   uninterpreted functions for free.
3. Has stable bindings we can call from both Python and Rust.
4. Is open source and actively maintained.

## Decision

Adopt Microsoft Z3 as Monomix's SMT backend, accessed through a single
internal API: `monomix.solver`.

In Phase 1 we use the `z3-solver` PyPI package directly from Python and
treat the Rust kernel as a producer of expression IR that Python lowers
to Z3 ASTs.

In Phase 2 we add a Rust crate `solver-bridge` that links `z3-sys` and
exposes the same `assume / prove / decide / model` API natively to the
Rust kernel, so hot paths don't have to round-trip through Python.

The Python and Rust facades stay shape-compatible — same verbs, same
return types — so callers above the bridge don't need to know which side
they're on.

## Alternatives considered

- **CVC5.** Comparable solver, often better on strings and quantifiers,
  weaker than Z3 on nonlinear real arithmetic (nlsat). nlsat is the
  feature we care about most for a CAS, so this loses on the dimension
  that matters here. Worth keeping the bridge polymorphic so we can add
  CVC5 later behind the same facade.
- **Yices2.** Fast on linear theories, weaker on nonlinear and on Python
  ergonomics.
- **MathSAT.** Closed source, license-incompatible with our intent to
  ship Monomix as open source.
- **Reimplementing redlog in Rust.** Large undertaking, distracts from
  the core CAS rewrite. Punt; revisit only if Z3 turns out to be
  insufficient for our workload.
- **No SMT backend, only algebraic decision procedures.** Forces us to
  reimplement nlsat and CAD ourselves. Out of scope for a solo project.

## Consequences

Positive:

- Decidable subproblems get a strong, well-tested decision procedure for
  free.
- Assumption tracking can be built on Z3's incremental `push/pop`
  instead of a hand-rolled assumption manager.
- Test cases written as SMT-LIB are portable and reproducible.

Negative / risks:

- Z3 is a large dependency (≈10 MB native lib). Acceptable for a CAS;
  flagged as an optional extra so users who only want symbolic
  manipulation don't pay for it.
- Transcendental functions (`sin`, `exp`, …) are not decidable. The
  translator must either declare them as uninterpreted (sound but
  incomplete) or refuse with a typed `Unsupported` error. We start with
  the uninterpreted-with-warning route and tighten later.
- Z3's nonlinear real engine can run for a long time on adversarial
  inputs. We expose a `timeout_ms` parameter on every decision call and
  return `Unknown` rather than blocking.

## API contract

The facade in `monomix.solver` exposes four verbs:

    assume(constraints)        -> SolverContext   # incremental
    prove(theorem, assumptions=...)  -> Proved | Refuted | Unknown
    decide(formula, assumptions=...) -> Sat(model) | Unsat | Unknown
    simplify(expr, assumptions=...)  -> expr      # under context

Every call accepts a `timeout_ms` and a `theory` hint
(`real | int | bv | mixed`). `Unknown` is a first-class return value,
never an exception, so the kernel can fall back to algebraic methods
without exception handling.

## Open questions

- How do we surface Z3's tactic combinators to power users without
  leaking Z3 into the public API? Tentative answer: a `tactic=` keyword
  taking opaque tokens registered by `monomix.solver`.
- Do we want proof reconstruction (Z3 can emit proofs)? Useful for a CAS
  that wants to *justify* its simplifications, deferred to a later ADR.
