# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project shape

Monomix is a modern computer algebra system inspired by REDUCE. The architecture is "Python on the surface, Rust underneath" — the user-facing API is Python via PyO3 + maturin, while the symbolic engine is a pure Rust kernel. The active code is split across:

- `rust/monomix-kernel/` — Phase 1 (MVP) symbolic kernel.
- `rust/monomix-py/` — PyO3 binding crate exposing the kernel to Python.
- `python/monomix/` — Python package. Provides `monomix.Expr` (Rust-backed handle), `monomix.Session`, module-level kernel functions (`simplify`, `df`, `expand`, `solve`, `sub`, `evaluate_numeric`), and the SMT bridge under `monomix.smt`.
- `rust/solver-bridge/` — Phase 2 sketch; **not buildable** yet (Z3 deps commented out). Don't try to build it.

Authoritative scope and phasing live in [SCOPE.md](SCOPE.md). Architectural decisions are in [decisions/](decisions/) (ADRs); design notes per subsystem are in [designs/](designs/). When deciding whether a feature belongs in this phase, consult SCOPE.md first — it explicitly enumerates what's in / out / deferred for each phase.

## Common commands

All `cargo` commands below should be run from `rust/monomix-kernel/` unless otherwise noted.

| Task | Command |
|------|---------|
| Build the kernel | `cargo build` |
| All kernel tests (unit + integration + proptest) | `cargo test` |
| Just lib unit tests | `cargo test --lib` |
| One module's tests | `cargo test --lib solve::` (use `::` to scope; bare names like `solve` are rejected) |
| One named test | `cargo test --lib <test_name>` |
| Golden corpus tests | `cargo test --test golden_tests` |
| Print golden VERIFIED/SMOKE/skipped summary | `cargo test --test golden_tests -- --nocapture` |
| Criterion benches | `cargo bench` |
| Fuzz a target | `cd fuzz && cargo fuzz run fuzz_parser` (also `fuzz_simplify`, `fuzz_diff`) |
| Lint | `cargo clippy -- -D warnings` |
| Format | `cargo fmt` |
| Build Python bindings (dev loop) | `cd python && maturin develop` |
| Python tests (Expr, Session, kernel calls, SMT bridge) | `cd python && pytest` |

The Rust workspace root is at the repo root. Member crates inherit `edition`, `license`, `authors`, and the `unsafe_code = "forbid"` lint from `[workspace.package]` / `[workspace.lints.rust]` in [Cargo.toml](Cargo.toml).

## Kernel architecture

The kernel exposes its public surface in [rust/monomix-kernel/src/lib.rs](rust/monomix-kernel/src/lib.rs) (parser, simplify, poly, solve, diff, substitute, evalnum, expr, error). Every other module is internal detail.

**Expression representation.** The kernel is built around a hash-consed DAG. All nodes live in a single `ExprPool` and are referred to by `ExprId` (a 32-bit handle). The pool dedups by content hash — two identical subtrees collapse to the same `ExprId`, so structural equality is a `==` on the handle. `ExprPool::new()` pre-interns `zero`, `one`, `minus_one`. The `ExprNode` enum (atoms + composites: `SmallInt`, `BigInt`, `Rational`, `Float`, `Symbol`, `Add`, `Mul`, `Pow`, `Neg`, `Div`, `Eq`, `Fn`, `List`, …) is subject to a **compile-time guard** that fails compilation if it exceeds 32 bytes. Don't widen variants past that without measuring; the guard is in [src/expr/mod.rs](rust/monomix-kernel/src/expr/mod.rs) (`_EXPR_NODE_SIZE_GUARD`).

**Identifiers are case-insensitive.** `ExprPool::intern_str` lowercases on insert. There is a fast path for ASCII-lowercase input that avoids the allocation; treat the public `intern_str_pub` as the supported entry point.

**Numeric atoms.** Symbolic rationals are the default. Integers narrow automatically (`pool.integer(BigInt)` returns `SmallInt` when the value fits in `i64`); `pool.rational(p, q)` normalizes sign and reduces by GCD, returning `SmallInt(0)` when `p == 0` and `Integer(p)` when `q == 1`. **`Float` atoms are produced only by explicit `evaluate_numeric`** — symbolic and floating-point representations are not silently mixed (per SCOPE.md §1.1). If new code introduces a Float into the symbolic IR for a fully-exact input, that's almost certainly a bug.

**Simplifier.** The driver in [src/simplify/driver.rs](rust/monomix-kernel/src/simplify/driver.rs) runs `map_bottom_up` over the DAG and applies rewrites until a fixed point (capped at `MAX_ITERS`). It memoizes via `SimplifyCache`, which is **keyed by `(registry_id, ExprId)`** — every `RuleRegistry` carries a process-monotonic id so cached entries can't leak across rule sets. If you change cache semantics, preserve this partitioning. `simplify` uses an empty `DEFAULT_RULES` registry; `simplify_trig` opts into the Pythagorean rule. The Pattern matcher supports subset matching against `Add` LHS patterns — a rule with `Add([sin(u)^2, cos(u)^2])` will collapse those two siblings even when other summands are present.

**Polynomial layer.** `view_mut(pool, expr, var)` reifies an `ExprId` into a sparse `UnivPoly` (`Vec<Term>`) in `var`; `to_expr` round-trips it back. `poly_div` and `poly_gcd` route new coefficients through `fold_numeric` to keep numeric `Div` clutter out of returned terms; `poly_gcd` monic-normalizes at every Euclidean step when the leading coefficient is numeric. The `is_unit` predicate downstream (in `simplify_div`) relies on canonical `pool.one` for the leading coefficient — don't reintroduce `Div(1,1)` shapes there.

**Solver.** `solve_system` runs an **exact-rational Gaussian elimination over `BigRational` first** (`try_solve_system_exact`); it falls back to f64 with partial pivoting only when a subterm fails to evaluate exactly (Float, transcendental, irrational symbol). The exact path is the default; preserve it. Quadratic solving uses an exact discriminant classifier (`classify_discriminant`) that reads sign directly from numeric atoms — avoid routing tiny rationals or huge BigInts through f64 sign checks.

**No-panic invariant.** The kernel must not `panic!` on user input. Returns are `Result<T, KernelError>` for anything user-facing. This is verified by `cargo-fuzz` (targets under `fuzz/fuzz_targets/`). Mind this when adding `unwrap`/`expect` on data derived from parsing or evaluation. `assert!` on internal invariants is OK; `assert!` on user data is not.

**No `unsafe`.** The workspace pins `unsafe_code = "forbid"`. Don't add `unsafe` — propose an alternative.

## Golden corpus convention

Manifests under [rust/monomix-kernel/tests/golden/](rust/monomix-kernel/tests/golden/) drive `tests/golden_tests.rs`. Each entry is `input` + `expected`, with optional `ignore = true` + `ignore_reason`. Two valid reasons for `ignore = true`:

1. **Unimplemented feature** — plain prose; flip to `false` when the feature lands.
2. **Intentional REDUCE divergence** — `ignore_reason` references an id in `tests/golden/divergences.toml`.

The runner tallies VERIFIED / parse-SMOKE / skipped per manifest and **asserts a per-manifest minimum verified count** (`MIN_VERIFIED_*` constants in `golden_tests.rs`). If you mark entries as ignored and drop below the floor, the test fails — either re-verify entries or bump the floor with a written rationale.

## Conventions worth knowing before changes

- **Module-graph direction:** `simplify::rational` depends on `poly`; `poly` depends on `simplify::numeric`. Don't introduce calls from `poly` into `simplify::driver` or `simplify::rational` — that creates a cycle.
- **Span tracking is opt-in per node.** The parser's `SpanMap` records `ExprId → Span` for nodes whose creation site has access to the source span. Adding new constructor paths (e.g. rational literals stitched together from two tokens) should also record spans so diagnostics can point at the whole literal.
- **`pool.div(x, one)` short-circuits** to `x` (no Div node). Useful for monic normalization.
- **Hash-cons cache invalidation hazard.** A `SimplifyCache` entry computed under one `RuleRegistry` is unsound under another. Re-using a cache across `simplify` and `simplify_trig` is only correct because the `(registry_id, ExprId)` key partitions them.
- **`view_mut` enforces `u32::MAX` as the exponent ceiling.** Truncating an exponent through `as u32` is a known footgun; the existing code rejects with `ExponentTooLarge`. Don't reintroduce a silent cast.
