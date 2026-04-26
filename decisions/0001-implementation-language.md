# ADR-0001: Implementation Language — Python with a Rust Extension Kernel

## Status

Accepted — 2026-04-26

## Context

The Monomix project — a modern CAS inspired by REDUCE — began with the working assumption that Rust would be the implementation language. This ADR revisits that assumption against the project's actual user-facing criteria, which were not previously written down: ease of installation, ease of integration with AI tooling (specifically MCP and Python-centric LLM frameworks), and a workable plugin story.

The CAS use cases this project targets, in priority order:

1. LLMs invoking symbolic computation via MCP.
2. Researchers and students using the system from notebooks or as a library.
3. Plugin authors extending the system with domain-specific algebra (physics, control theory, etc.).
4. Embedding as a library inside other applications.

A pure-Rust implementation serves (1) acceptably via MCP but creates real friction for (2) and (3). Jupyter integration requires PyO3 in any case; plugin systems in Rust are uniformly poor for end-user extension (native dylibs are ABI-fragile, WASM adds toolchain overhead for plugin authors, and embedding a scripting language reintroduces exactly the tradeoff this decision is trying to resolve).

A pure-Python implementation serves (2) and (3) excellently but has performance ceilings that matter for symbolic computation. Tree walks, structural equality, and term rewriting can run 50-100× slower in pure Python than in equivalent Rust. SymPy is a real existence proof that pure Python works, but also a cautionary one — its performance limits are well documented and have driven serious users toward Mathematica and Maple.

## Decision

Use **Python as the primary implementation language with a Rust extension module for the expression kernel**, distributed as a wheel via `maturin` and `PyO3`.

The split:

- **Rust (kernel):** expression representation (hash-consed DAG), big-integer and rational arithmetic, the parser, the term-rewriting engine that drives the simplifier, polynomial primitives, and any other CPU-bound inner loop.
- **Python (everything else):** public API surface, CLI/REPL, MCP server, plugin system, documentation, high-level integration tests, and integration with notebooks and the AI ecosystem.

The Rust kernel exposes its types as Python classes via PyO3. Python is the boundary the user touches. End users never need a Rust toolchain.

## Consequences

### Positive

- **Installation:** `pip install monomix` or `uv add monomix`. Wheels prebuilt for Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64. No Rust toolchain required for users.
- **AI integration:** Native. The MCP server is a Python module using the official `mcp` Python SDK. Notebook users get real Python objects. LangChain, LlamaIndex, and similar frameworks integrate as plain Python.
- **Plugin system:** Python entry points (`monomix.plugins` group). Plugins are pure Python packages that register handlers for new functions, rewrite rules, or pretty-printers. Zero ABI concerns, zero toolchain requirement for plugin authors.
- **Performance where it matters:** The Rust kernel runs at native speed and releases the GIL during long operations, so the MCP server can handle concurrent requests without Python-side serialization.
- **Contributor pool:** Python contributors vastly outnumber Rust contributors in math and science. Plugin authors do not need Rust at all.
- **Iteration speed:** High-level logic iterates at Python speed with no rebuild. Rust changes require `maturin develop` (~5-15 seconds incremental).
- **Escape hatch:** A pure-Python fallback path can be added later if the Rust kernel turns out not to pay for itself, without breaking the public API.

### Negative

- **Two languages.** The maintainer must be fluent in both. Where the boundary sits between them is an ongoing design question.
- **Build complexity.** CI builds wheels for multiple platforms. Local development needs both a Python toolchain and `cargo`. `maturin` makes this manageable but not trivial.
- **API surface tax.** Every Rust type exposed to Python requires PyO3 boilerplate. Adding a new kernel feature is more work than adding a pure-Python or pure-Rust feature would be.
- **Debugging across the boundary.** Stack traces crossing PyO3 are noisier. Standard tooling is `RUST_BACKTRACE=1` plus Python tracebacks.
- **Linux distribution complexity.** `manylinux` wheels must be built inside the official Docker images. `cibuildwheel` handles this but is its own learning curve.

### Neutral

- **Boundary-crossing overhead.** ~100ns per PyO3 call. At the granularity actually used (operations over whole expression trees, not per-node calls), this is negligible.
- **Python startup time.** Mitigated by keeping the CLI's top-level import surface small and lazy-loading heavy submodules.

## Alternatives Considered

### Pure Rust (the original choice — rejected)

Rejected because Rust plugin systems are poor for end-user extension, and AI/notebook integration ends up requiring PyO3 anyway. If PyO3 is going to be on the critical path, leading with Python on the user-facing side is strictly better.

### Pure Python or fork SymPy (rejected, with caveat)

Rejected because the project's stated motivation is to rewrite REDUCE with modern languages and technology. Forking SymPy would not satisfy that goal, and pure-Python performance ceilings on symbolic computation are well documented.

Caveat: pure Python remains the right escape hatch if the Rust kernel proves not to pay for itself. The Python layer is designed so the kernel is replaceable by a pure-Python fallback for development on platforms where Rust toolchains are awkward.

### Julia (rejected)

The best technical fit on its own merits — multiple dispatch is the natural paradigm for CAS, `Symbolics.jl` is excellent, performance is comparable to Rust. Rejected because:

1. AI integration is via `PyCall`, not native. The MCP and LLM ecosystems are Python-first.
2. The contributor pool for a solo-maintainer open-source CAS is materially smaller than Python's.
3. Distribution is awkward compared to `pip install`.

Julia would be the right choice for a research project whose primary users are scientists running it interactively. It is not the right choice when the primary integration target is the Python AI ecosystem.

### TypeScript/Node, Go, OCaml, Common Lisp (rejected)

- **TypeScript/Node:** poor numerical/symbolic performance ceiling; awkward big-integer story.
- **Go:** no operator overloading and no pattern matching — both make CAS code painful.
- **OCaml:** excellent for tree manipulation, ecosystem too small for the AI integration goal.
- **Common Lisp:** the best technical fit historically (REDUCE is Lisp-native), but explicitly out of scope per the project's design principle of avoiding a Lisp runtime.

## Implementation Notes

- Project layout follows the standard `maturin` mixed Python/Rust structure: `python/monomix/` for the Python package, `src/` for the Rust kernel, `Cargo.toml` and `pyproject.toml` at the repo root.
- Minimum Python: 3.11 (for improved error messages, `Self`, `tomllib`).
- Minimum Rust: pinned in `rust-toolchain.toml`.
- Wheels are built and tested in CI via `cibuildwheel` against CPython 3.11, 3.12, and 3.13 on Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64.
- Pure-Python fallback path: optional. If implemented, gated behind `MONOMIX_PURE_PYTHON=1` for development on unsupported platforms.

## References

- PyO3: https://pyo3.rs/
- Maturin: https://www.maturin.rs/
- `cibuildwheel`: https://cibuildwheel.pypa.io/
- Polars: example of a successful Python-Rust hybrid (https://pola.rs/)
- Pydantic v2: example of a successful Python-Rust hybrid (https://docs.pydantic.dev/)
- MCP Python SDK: https://github.com/modelcontextprotocol/python-sdk
