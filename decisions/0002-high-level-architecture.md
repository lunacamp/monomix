# ADR-0002: High-level architecture

**Status:** Proposed
**Date:** 2026-04-26
**Deciders:** Roman (solo maintainer)
**Supersedes:** None
**Depends on:** ADR-0001 (implementation language)

## Context

Monomix is a modern computer algebra system inspired by REDUCE. ADR-0001 established the Python + Rust kernel split. This ADR documents the high-level architecture that follows from that decision: how the system is layered, how data flows across the PyO3 boundary, and where each responsibility lives.

The driving forces are:

- **Installability:** `pip install monomix` must work without a Rust toolchain (pre-built wheels).
- **AI ecosystem integration:** Jupyter, MCP, LangChain — all Python-native.
- **Performance:** Symbolic tree manipulation, polynomial arithmetic, and parsing must run at native speed.
- **Extensibility:** Third-party plugins as plain Python packages, discoverable via entry points.
- **Concurrency:** The MCP server (Phase 1.5) needs to handle concurrent requests; the kernel must not hold the GIL.
- **Solo maintainer:** The architecture must be tractable for one person to build, test, and ship incrementally.

## Decision

Monomix is a three-tier system:

### Tier 1 — Python surface layer

Everything the user touches is Python. This tier owns:

- **CLI / REPL** (`monomix` console script): built on `prompt_toolkit` + `rich`. Parses multi-line input (`;` / `$` terminators), dispatches to the kernel, formats output.
- **Public API** (`monomix.Expr`): Python wrapper around opaque kernel handles. Implements `__add__`, `__mul__`, etc. so expressions compose with normal Python operators.
- **Session state** (`monomix.Session`): mutable binding environment (`:=` assignment, `clear`). The kernel is stateless; all mutable state lives here.
- **Plugin host**: discovers plugins via the `monomix.plugins` entry-point group at `Session` construction. Plugins register functions, rewrite rules, and pretty-printers. The standard function library is itself a plugin, exercising this contract from Phase 1.
- **Documentation**: Sphinx + myst-parser, hosted on Read the Docs. Doctests on every public function.

Design rules for this tier:
- No symbolic computation logic — only glue, formatting, and state management.
- All kernel calls go through PyO3 wrappers; no raw `ctypes` or `cffi`.
- Errors from the kernel arrive as Python exceptions rooted at `MonomixError`.

### Tier 2 — PyO3 boundary

A thin translation layer, not a tier with its own logic. Responsibilities:

- **Type conversion**: Python `int` ↔ Rust `num-bigint::BigInt`; Python `str` → Rust `&str` for parsing; opaque `Expr` handles wrap `Arc<ExprNode>`.
- **GIL management**: release the GIL on any operation expected to take >1 ms (parsing, simplification, polynomial work, solving). This is what lets the Phase 1.5 MCP server handle concurrent requests.
- **Error mapping**: Rust `Result<T, KernelError>` → Python exception hierarchy (`ParseError`, `EvalError`, `UnsupportedError`).
- **Zero-copy where possible**: string slices for parsing input; `Arc` cloning (not deep copy) for expression handles.

The boundary is defined by a single Rust crate (`monomix-py`) that depends on the kernel crate (`monomix-kernel`). `maturin` builds the combined extension module.

### Tier 3 — Rust kernel

All symbolic computation lives here. The kernel is a pure Rust library with no Python dependency — it can be tested, benchmarked, and fuzzed independently via `cargo`.

Components:

| Component | Responsibility | Key types |
|-----------|---------------|-----------|
| **Expression DAG** | Hash-consed directed acyclic graph of expression nodes. Shared subexpressions are stored once. Equality is pointer comparison after consing. | `ExprNode`, `ExprPool`, `Arc<ExprNode>` |
| **Parser** | REDUCE-syntax subset → AST with byte-accurate source spans. Error recovery so one bad statement doesn't abort a multi-statement input. | `parse(src) -> Result<Ast, Vec<Diagnostic>>` |
| **Simplifier** | Rewrite engine: like-term collection, identity elimination, power rules, common-factor cancellation, Pythagorean identity. Intentionally narrow in Phase 1; generalized in Phase 2. | `simplify(expr, &ExprPool) -> ExprNode` |
| **Differentiation** | `df(expr, var)`: constants, variables, sums, products, quotients, powers, chain rule, standard functions. | `differentiate(expr, var, &ExprPool) -> ExprNode` |
| **Polynomial ops** | Sparse univariate representation. Add, sub, mul, exp, div-with-remainder, degree, coefficient extraction. | `UnivPoly`, `poly_div`, `expand`, `collect` |
| **Equation solver** | Linear, quadratic, and n×n linear systems via Gaussian elimination with partial pivoting. Returns symbolic solutions or empty set with diagnostics. | `solve(equation, var) -> Vec<ExprNode>` |
| **Substitution** | Tree walk replacing symbols with expressions. `evaluate_numeric` is the only path that produces floats. | `substitute`, `evaluate_numeric` |
| **Numeric backend** | `num-bigint` for arbitrary-precision integers, `num-rational` for exact rationals. No GMP/LGPL dependency. Conversion to/from Python `int` happens once at the PyO3 boundary. | `BigInt`, `BigRational` |

Design rules for this tier:
- All types are `Send + Sync`.
- No `unsafe` outside the PyO3 boundary crate.
- No I/O, no filesystem access, no networking. The kernel is a pure function library.
- Errors use `Result<T, KernelError>` — never `panic!` on user input (verified by `cargo-fuzz`).

### Extension points

Two extension mechanisms attach to the Python surface layer:

- **MCP server** (Phase 1.5): a separate Python process (`monomix-mcp`) using the official `mcp` Python SDK. It imports `monomix` as a library and exposes tools (`differentiate`, `solve`, `simplify`, etc.) over the MCP protocol. Because the kernel releases the GIL, concurrent MCP requests run in parallel.
- **Plugins** (Phase 1): plain Python packages that register via the `monomix.plugins` entry-point group. A plugin can register functions, rewrite rules, and pretty-printers. Discovery happens at `Session` construction.

### Crate / package layout

```
monomix/
├── Cargo.toml              # workspace: kernel + py crates
├── pyproject.toml           # maturin build, Python metadata
├── crates/
│   ├── monomix-kernel/      # pure Rust library (Tier 3)
│   │   ├── src/
│   │   │   ├── expr.rs      # ExprNode, ExprPool, hash-consing
│   │   │   ├── parser.rs    # REDUCE-subset parser
│   │   │   ├── simplify.rs  # rewrite engine
│   │   │   ├── diff.rs      # symbolic differentiation
│   │   │   ├── poly.rs      # univariate polynomial ops
│   │   │   ├── solve.rs     # equation solver
│   │   │   ├── subst.rs     # substitution + numeric eval
│   │   │   ├── numeric.rs   # BigInt/BigRational wrappers
│   │   │   └── error.rs     # KernelError enum
│   │   ├── benches/         # criterion benchmarks
│   │   └── tests/           # integration tests + proptest
│   └── monomix-py/          # PyO3 boundary crate (Tier 2)
│       └── src/
│           ├── lib.rs        # #[pymodule] entry point
│           ├── expr.rs       # Python Expr wrapper
│           └── error.rs      # KernelError → Python exception
├── python/
│   └── monomix/             # pure-Python surface (Tier 1)
│       ├── __init__.py       # public API re-exports
│       ├── session.py        # Session, bindings, plugin discovery
│       ├── cli.py            # REPL (prompt_toolkit + rich)
│       ├── plugins.py        # plugin protocol and host
│       └── mcp_server.py     # MCP server (optional extra)
├── tests/                    # pytest + hypothesis tests
│   ├── test_expr.py
│   ├── test_session.py
│   ├── test_golden/          # legacy .tst/.rlg corpus
│   └── conftest.py
├── docs/                     # Sphinx + myst-parser
├── decisions/                # ADRs
└── legacy/                   # reference REDUCE source (read-only)
```

### Data flow for a typical request

1. User types `df(x^3 + sin(x), x);` in the CLI.
2. Python `Session` passes the source string across the PyO3 boundary.
3. Rust parser produces an AST with spans; PyO3 maps errors to `ParseError`.
4. Rust `differentiate` walks the hash-consed DAG, producing a new DAG node (`3*x^2 + cos(x)`).
5. The result is returned as an opaque `Arc<ExprNode>` handle wrapped in a Python `Expr`.
6. Python `Session` formats the `Expr` for display using `rich`.
7. The user sees `3*x^2 + cos(x)`.

GIL is released during steps 3-4. If this were an MCP request, other requests could execute concurrently during that window.

## Options Considered

### Option A: Monolithic Rust with Python bindings (current decision)

| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium — two languages, but clear boundary |
| Install UX | Excellent — pre-built wheels, `pip install` |
| Performance | Excellent — kernel is pure Rust |
| AI integration | Excellent — native Python imports |
| Plugin authoring | Excellent — plain Python packages |
| Solo-maintainer tractability | Good — clear separation of concerns |

**Pros:** Best of both worlds. Python users get `pip install`. Rust kernel gets full native performance. Plugin authors write Python. MCP server is trivial (Python library call).

**Cons:** Two-language build. PyO3 boundary requires care (GIL, type conversion). Debugging spans two worlds.

### Option B: Pure Rust with a generated Python wrapper

| Dimension | Assessment |
|-----------|------------|
| Complexity | Lower in one sense (single language), higher in another (FFI generation) |
| Install UX | Worse — `maturin` still needed, but all logic is Rust |
| Performance | Excellent |
| AI integration | Poor — Python wrapper is thin, Pythonic API is awkward |
| Plugin authoring | Poor — plugins must be Rust or cross FFI |
| Solo-maintainer tractability | Lower — Rust for glue code is slower to iterate |

**Pros:** Single-language kernel. No Python logic to test.

**Cons:** Plugin system becomes Rust-only or requires a second FFI layer. MCP server must be Rust (less mature SDK). Jupyter integration is awkward. The "Python wrapper" ends up reimplementing half of Tier 1 in Rust anyway.

### Option C: Pure Python with optional Cython/C extensions

| Dimension | Assessment |
|-----------|------------|
| Complexity | Low |
| Install UX | Excellent |
| Performance | Poor for tree manipulation; Cython helps but adds build complexity |
| AI integration | Excellent |
| Plugin authoring | Excellent |
| Solo-maintainer tractability | Good initially, performance ceiling hits early |

**Pros:** Simplest to start. Largest contributor pool. No two-language debugging.

**Cons:** Performance ceiling on symbolic computation is real — SymPy's performance issues are well-documented. Hash-consing and GCD in pure Python is 10-100x slower than Rust. Cython bridges the gap partially but adds its own build complexity without the ergonomics of Rust's type system.

## Trade-off Analysis

The core trade-off is **development velocity vs. runtime performance**. Option A accepts a moderate increase in build complexity (two languages, PyO3 boundary) in exchange for native-speed symbolic computation and first-class Python ergonomics. The boundary is deliberately thin — a single crate with type conversions and GIL management — so the complexity tax is bounded.

The second trade-off is **plugin ecosystem reach vs. kernel safety**. By keeping plugins in Python and the kernel in Rust, we get the largest possible plugin-author pool while keeping the performance-critical path in a memory-safe, high-performance language. The downside is that plugins cannot extend the kernel's inner loop — they operate at the Python tier. This is acceptable for Phase 1-2; if kernel-level extensions become necessary, a Rust plugin API can be added later.

## Consequences

- **Easier:** Installing, extending via plugins, integrating with Jupyter/MCP/LangChain, writing and running tests (pytest for Python, cargo test for Rust independently).
- **Harder:** Debugging across the PyO3 boundary, CI wheel matrix (5 platforms × 3 Python versions), onboarding contributors who know only one language.
- **Revisit later:** Whether kernel-level (Rust) plugins are needed; whether `ExprPool` should be shared across sessions for caching; whether the hash-consing strategy needs generational collection for long-running sessions.

## Action Items

1. [ ] Create the Cargo workspace with `monomix-kernel` and `monomix-py` crates
2. [ ] Implement `ExprNode` and `ExprPool` with hash-consing (§0.2)
3. [ ] Set up PyO3 boundary with `Expr` wrapper and GIL release
4. [ ] Implement the REDUCE-subset parser with span information (§0.6)
5. [ ] Set up `maturin` build + `cibuildwheel` CI for the §0.9 platform matrix
6. [ ] Create the Python `Session` and plugin discovery mechanism
7. [ ] Wire up the CLI REPL with `prompt_toolkit` + `rich`
8. [ ] Establish the four-layer test strategy (§0.7)
