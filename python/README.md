# `monomix` (Python package)

Python distribution for Monomix. Exposes the Rust kernel through PyO3
plus a small Python facade for sessions, bindings, and an SMT bridge
protocol.

## Layout

    python/
      pyproject.toml            -- maturin build config
      monomix/
        __init__.py             -- public surface (Session, Expr, simplify, df, …)
        _kernel.pyi             -- type stubs for the PyO3 module
        errors.py               -- exception hierarchy re-exported from the kernel
        session.py              -- Session wrapper around the kernel handle
        smt/
          __init__.py           -- public SMT-bridge API
          translate.py          -- Backend protocol + Translator
          results.py            -- Proved / Refuted / Sat / Unsat / Unknown
          errors.py             -- TranslationError, Unsupported, …
      tests/
        test_expr.py            -- operator overloading and inspection
        test_session.py         -- Session lifetime, bindings, sort declarations
        test_kernel_calls.py    -- simplify / df / expand / solve / sub / evaluate_numeric
        test_gil.py             -- soft-floor parallelism check

## Quick start

    cd python
    pip install maturin
    maturin develop
    pytest

`maturin develop` builds the Rust kernel and installs the `monomix`
package into the current Python environment.

## SMT bridge

`monomix.smt` ships the `Backend` protocol, the `Translator`, and the
solver-agnostic result vocabulary (`Proved`, `Refuted`, `Sat`, …). No
backend is bundled — users wire their own adapter around whichever
SMT solver they want to drive. See [`../designs/smt.md`](../designs/smt.md)
for the protocol contract and the feature requirements any conforming
backend must implement.
