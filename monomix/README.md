# Monomix — SMT solver integration

This subtree contains the scaffolding for delegating decidable subproblems
from the Monomix CAS kernel to an SMT solver (initially Microsoft's Z3).

The CAS kernel itself does not need to reimplement decision procedures for
nonlinear real arithmetic, bit-vector arithmetic, or quantifier-free linear
integer arithmetic. Instead, it lowers a typed subset of its expression IR
into SMT-LIB and asks Z3.

## Layout

    monomix/
      README.md                     -- this file
      docs/
        adr/
          0001-z3-as-smt-backend.md -- architecture decision record
      python/
        pyproject.toml
        monomix/
          expr.py                   -- placeholder expression IR
          solver/
            __init__.py             -- public facade (assume / prove / decide / model)
            translate.py            -- Monomix IR <-> Z3 AST
            z3_backend.py           -- Z3 wrapper, push/pop, tactics
            errors.py
        tests/
          test_solver.py            -- runnable end-to-end demos
      rust/
        solver-bridge/
          Cargo.toml                -- crate stub (not yet wired into a workspace)
          src/lib.rs                -- FFI sketch & integration notes
          README.md

## Phasing

Phase 1 (this scaffold): Python-only integration via the `z3-solver` PyPI
package. Good enough for prototyping, expression translation, capability
gating, and exercising the API surface against real Z3.

Phase 2 (later): native Rust kernel calls through `z3-sys` directly, with
the same translate/decide API. The Rust crate under `rust/solver-bridge/`
documents the eventual shape; it's a sketch, not yet buildable.

## Quick start (Phase 1)

    cd monomix/python
    pip install -e .[dev]
    pytest

The tests in `tests/test_solver.py` double as runnable usage examples.
