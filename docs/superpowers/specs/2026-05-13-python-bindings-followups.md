# Python bindings — follow-up notes

Doc-debt and deferred items surfaced while implementing the
python-bindings spec.

## ADR-0002 inconsistencies

- `decisions/0002-high-level-architecture.md` says expression handles
  are `Arc<ExprNode>`; the kernel actually uses an arena pool with
  `ExprId` handles. The PyO3 boundary therefore holds
  `(Arc<Mutex<ExprPool>>, ExprId)`, not `Arc<ExprNode>`.
- ADR-0002 says the crate layout is `crates/monomix-kernel/` and
  `crates/monomix-py/`; the actual layout is `rust/monomix-kernel/`
  and `rust/monomix-py/`.

**Action:** write a follow-up ADR amending these two points, citing
this work as the reason for the correction.

## Deferred Phase 1 items

The Python bindings work does not include:

- Plugin entry-point discovery (Phase 1 §1.10).
- CLI / REPL (Phase 1 §1.9).
- CI wheel matrix (SCOPE §0.9 — needs its own spec).
- Sphinx / Read the Docs setup.

## Build hygiene

- `maturin develop` on Windows drops `python/monomix/_kernel.pyd` and
  `python/monomix/monomix_py.pdb` next to the source. They aren't
  tracked, but they show up in `git status` after every dev rebuild.
  Worth a `.gitignore` entry (`*.pyd`, `*.pdb`) so they stop appearing
  in untracked-file noise.

## Out-of-scope items called out during brainstorming

- Reverse `model → Expr` reconstruction in the SMT bridge (`Refuted`
  and `Sat` return raw Python `int` / `Fraction` / `bool` values).
- A shipped reference backend. The source has the protocol and
  Translator only; integrators wire their own backend per
  `designs/smt.md`.
- REDUCE-syntax extensions for inequalities / boolean operators (the
  new kernel variants are only reachable via Python constructors).

## Known design hazards documented in the user-facing docs

- Operator precedence with `==` vs `&` / `|`. Documented; no automated
  check.
- `__bool__` of non-`Eq` expressions raises. Aligned with NumPy
  convention; documented.

## Code-review findings (PR #3)

Issues surfaced reviewing the bindings. The normalization bug was fixed
in this branch; the rest are deferred with the rationale below.

### Fixed

- **Single-element `And` / `Or` from dedup.** `ExprPool::and_` / `or_`
  ran the empty/singleton short-circuits *before* `sort` + `dedup`, so
  duplicate operands that deduped down to one (e.g. `(x<y) & (x<y)`)
  interned a degenerate `And([x])` instead of collapsing to `x`. Fixed
  by reordering dedup before the length checks; covered by
  `and_dedup_to_single_operand_collapses` / `or_dedup_…` in
  `rust/monomix-kernel/src/expr/mod.rs`.

### Deferred

- **`solve` drops the solved-for variable.** `kernel_fns::solve`
  flattens every substitution to its values
  (`flat_map(|subst| subst.into_iter().map(|(_var, val)| …))`),
  discarding which variable each value binds. Correct for Phase 1
  single-variable solving (one var per solution → list of roots), but
  multi-variable solving will need a richer return shape
  (e.g. `list[dict[Expr, Expr]]`). Revisit when systems-solving is
  exposed to Python.
- **`bool` coerces to `int` in operators.** `coerce_to_expr` tries
  `BigInt` before `f64`, and Python `bool` is an `int` subclass, so
  `x + True` silently becomes `x + 1`. Harmless today; if a real
  Boolean atom path is ever wanted from Python arithmetic, intercept
  `bool` explicitly before the integer branch.
- **`.expect("pool mutex poisoned")` panics across FFI.** Every pool
  lock unwraps. A poisoned mutex (only reachable if a thread already
  panicked while holding the lock) surfaces as a PyO3 panic/abort
  rather than a clean `MonomixError`. The no-panic invariant covers
  kernel-on-user-input, not lock poisoning, so risk is low — but if we
  ever want a graceful story, map `PoisonError` to a `MonomixError`.
- **`__hash__` / `__eq__` contract is non-standard.** `__eq__` returns
  a symbolic `Eq` node and relies on `__bool__` for dict-key resolution
  (SymPy-style). It works because hash-consing makes `__bool__` an
  id-compare, but it is fragile: anything that calls `==` expecting a
  `bool` gets an `Expr`. Documented as a hazard; no automated guard.
