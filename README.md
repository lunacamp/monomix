# Monomix

Monomix is a modern computer algebra system inspired by REDUCE. The user
surface is Python; the inner kernel will be Rust (PyO3 + maturin). The
goal: `pip install monomix`, native-speed symbolic computation, and
first-class integration with the Python AI ecosystem (Jupyter, MCP,
LangChain).

**Status: planning + early scaffolding.** The architectural decisions are
captured in `decisions/`; design notes for individual subsystems are in
`designs/`. The code in tree today is the Rust symbolic kernel
(`rust/monomix-kernel/`) and its Python bindings (`rust/monomix-py/`,
`python/`); see `python/README.md`.

For the full project plan, phase breakdown, and in-scope/out-of-scope
boundaries, read `SCOPE.md`.

## Repo map

    .
    ├── README.md            -- this file
    ├── SCOPE.md             -- project scope and phase plan
    ├── LICENSE              -- MIT
    ├── Cargo.toml           -- Rust workspace root
    ├── decisions/           -- ADRs (numbered 0001, 0002, ...)
    ├── designs/             -- subsystem design notes
    ├── python/              -- the `monomix` Python package
    │   ├── pyproject.toml
    │   ├── monomix/
    │   └── tests/
    ├── rust/                -- Rust crates (workspace members)
    │   ├── monomix-kernel/  -- Phase-1 symbolic kernel
    │   └── monomix-py/      -- PyO3 bindings to the kernel
    └── legacy/              -- REDUCE r7357 source, gitignored,
                                used as a correctness oracle

## Quick start

    cd python
    pip install -e .[dev]
    pytest

## License

MIT — see `LICENSE`.
