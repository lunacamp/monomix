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

- Reverse `model → Expr` reconstruction in the SMT bridge (it
  currently returns raw Python `int` / `Fraction` for numeric values).
- Additional SMT backends beyond Z3.
- REDUCE-syntax extensions for inequalities / boolean operators (the
  new kernel variants are only reachable via Python constructors).

## Known design hazards documented in the user-facing docs

- Operator precedence with `==` vs `&` / `|`. Documented; no automated
  check.
- `__bool__` of non-`Eq` expressions raises. Aligned with NumPy
  convention; documented.

## Pyright suppressions worth revisiting

- `python/monomix/smt/z3_backend.py` carries a file-level
  `# pyright: reportOptionalMemberAccess=false` because `z3` may be
  `None` when the package is missing. A cleaner approach is to wrap
  the import in a small typed shim that asserts non-`None` once at
  module init, so per-call accesses can be type-checked normally.
