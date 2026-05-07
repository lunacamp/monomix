# `monomix` (Python package)

This is the Python distribution for Monomix. Today it ships only the SMT
bridge — the Phase 1 expression IR, parser, simplifier, and Rust kernel are
not yet implemented (see `../SCOPE.md`).

The CAS kernel does not need to reimplement decision procedures for
nonlinear real arithmetic, bit-vector arithmetic, or quantifier-free linear
integer arithmetic. It lowers a typed subset of its expression IR into
SMT-LIB and asks Z3. See `../decisions/0003-z3-as-smt-backend.md` for the
ADR.

## Layout

    python/
      pyproject.toml
      monomix/
        __init__.py
        expr.py                 -- placeholder expression IR
        solver/
          __init__.py           -- public facade (assume / prove / decide / model)
          translate.py          -- Monomix IR <-> Z3 AST
          z3_backend.py         -- Z3 wrapper, push/pop, tactics
          errors.py
      tests/
        test_solver.py          -- runnable end-to-end demos

## Phasing

Phase 1 (this scaffold): Python-only integration via the `z3-solver` PyPI
package. Good enough for prototyping, expression translation, capability
gating, and exercising the API surface against real Z3.

Phase 2 (later): native Rust kernel calls through `z3-sys` directly, with
the same translate/decide API. The Rust crate under `../rust/solver-bridge/`
documents the eventual shape; it's a sketch, not yet buildable.

## Quick start

    cd python
    pip install -e .[dev]
    pytest

The tests in `tests/test_solver.py` double as runnable usage examples.
