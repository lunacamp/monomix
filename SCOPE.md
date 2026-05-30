# Monomix: Scope Definition

**Project:** Monomix — a modern computer algebra system inspired by REDUCE (Python + Rust kernel implementation)
**Status:** Planning phase
**Last Updated:** 2026-04-26
**Maintainer:** Roman (solo)
**Stack:** Python 3.11+ on the surface, Rust extension kernel for hot paths (PyO3 + maturin)

## Executive Summary

This document defines the scope of the REDUCE rewrite across three development phases plus a foundational decisions section that must be settled before implementation begins. The goal is to build a modern, extensible computer algebra system that is easy to install (`pip install monomix`), integrates natively with the Python AI ecosystem (Jupyter, MCP, LangChain, etc.), supports plugins as plain Python packages, and delivers native-speed performance on its inner loops via a Rust extension kernel.

**Architecture in one sentence:** Python is the boundary the user touches; Rust is the engine they don't.

**Why this stack:** Documented in detail in `decisions/0001-implementation-language.md`. Briefly: a pure-Rust implementation creates real friction for Python-ecosystem AI integration and plugin authoring; a pure-Python implementation has performance ceilings that matter for symbolic computation. The hybrid wins on every user-facing criterion (install, AI integration, plugins, contributor pool) while keeping native performance where it pays — the expression kernel.

**Philosophy:** Settle load-bearing architectural decisions first, then ship a focused MVP (Phase 1), add an MCP layer once the engine is stable (Phase 1.5), expand strategically (Phase 2), and treat specialized features as opt-in (Phase 3+). Each phase is a complete, shippable product.

**Solo project note:** Decisions described as "team consensus" in earlier drafts have been replaced with "maintainer decision, documented as an ADR in `decisions/`." This is a single-maintainer project; pretending otherwise distorts the timeline.

---

## Phase 0: Foundational Decisions (Weeks 1-2)

These decisions are load-bearing for everything that follows. They must be made — and recorded as ADRs in `decisions/` — before any Phase 1 implementation begins. Each is summarized here with the recommended default; the ADR is where the reasoning lives.

### 0.1 Implementation Language and Architecture
- **Decision:** Python primary surface with a Rust extension kernel via PyO3 + maturin.
- **ADR:** `decisions/0001-implementation-language.md` (accepted 2026-04-26).
- **Why it matters:** This is the foundational choice that shapes every other decision below.

### 0.2 Internal Expression Representation
- **Decision required:** Tree, DAG (with hash-consing), or sum-of-products canonical form?
- **Recommended default:** Hash-consed DAG implemented in the Rust kernel, exposed to Python as opaque handles wrapping `Arc<ExprNode>`. Trees are simpler but blow up on shared subexpressions; full canonical forms are too restrictive for MVP.
- **Why it matters:** Determines memory model, equality testing strategy, and the simplifier's API surface.

### 0.3 Numeric Backends
- **Decision required:** Where does big-integer arithmetic live, and what backend?
- **Recommended default:** At the Python boundary, expose Python's native `int` (already arbitrary precision). Inside the Rust kernel, use `num-bigint` (pure-Rust, no GMP/LGPL friction). Conversion happens once at the boundary; the kernel never sees `PyLong` directly.
- **Why it matters:** Affects build matrix complexity, licensing, platform support, and per-call overhead at the Python/Rust boundary.

### 0.4 Error Model
- **Decision required:** How do Rust kernel errors surface in Python?
- **Recommended default:** Rust uses `Result<T, KernelError>` everywhere user input touches; PyO3 maps `KernelError` variants onto a small Python exception hierarchy rooted at `monomix.MonomixError` (subclasses: `ParseError`, `EvalError`, `UnsupportedError`). Errors carry source spans for parser diagnostics.
- **Why it matters:** Shapes both the public Python API and the MCP error contract.

### 0.5 Concurrency Posture
- **Decision required:** Does the kernel release the GIL? Is it `Send + Sync`?
- **Recommended default:** Yes to both. The Rust kernel's expression types are `Send + Sync`; PyO3 wrappers release the GIL on any operation expected to take >1ms (parsing, simplification, polynomial work). This is what lets the Phase 1.5 MCP server handle concurrent requests cleanly.
- **Why it matters:** Retrofitting GIL release and thread-safety later is significantly harder than designing for it.

### 0.6 REDUCE-Syntax Subset
- **Decision required:** Enumerate the grammar productions supported in MVP, and where the parser lives.
- **Recommended default for parser location:** Rust kernel. The parser produces an AST with span information; Python consumes the AST as opaque handles. This gets us native-speed parsing of large script files and clean diagnostic spans.
- **Recommended default for Phase 1 grammar:** expressions, operator precedence, function calls, `:=` assignment to symbols, `;` and `$` statement terminators, comments (`%` to end-of-line and `comment ... ;`). Excluded: `procedure`, `let`-rules, `for`/`while`/`do`, `operator`/`precedence` declarations.
- **Why it matters:** "REDUCE syntax (subset)" is too vague to test against; an explicit grammar is required.

### 0.7 Test Strategy
- **Decision required:** How is correctness verified across two languages?
- **Recommended default:** Four layers:
  - (a) `cargo test` unit tests inside the Rust kernel.
  - (b) `proptest` property-based tests inside the Rust kernel for low-level invariants (e.g., hash-cons equality, big-int arithmetic).
  - (c) `pytest` + `hypothesis` on the Python boundary for high-level invariants (e.g., `simplify(expand(e)) ≡ simplify(e)`, `df(integrate(f, x), x) ≡ f` once Phase 2 lands).
  - (d) Golden-output tests against a curated subset of the legacy `.tst`/`.rlg` files in `legacy/reduce-algebra-code-r7357-trunk/packages/*/`, run via `pytest`.
- **Why it matters:** Test count alone is a vanity metric. Layered tests with a real reference oracle catch the bugs hand-written tests miss.

### 0.8 Tooling Baseline
- **Build:** `maturin` for Python+Rust packaging; `cibuildwheel` in CI for the wheel matrix.
- **Python:** `uv` for dependency management; `ruff` for lint+format; `pyright` (strict mode) for type checking; `pytest` + `hypothesis` for tests.
- **Rust:** `cargo fmt`, `cargo clippy -- -D warnings`, `cargo audit`, `cargo deny check`; `criterion` for benchmarks.
- **Versions:** Minimum Python 3.11 (for `Self`, `tomllib`, improved error messages); MSRV pinned in `rust-toolchain.toml`.
- **CI gates:** all of the above must pass; wheels build successfully for the platform matrix in §0.9.

### 0.9 Distribution Matrix
- **Wheels built and tested in CI for:** Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64, Windows x86_64. Python 3.11, 3.12, 3.13.
- **Source distribution (`sdist`):** published to PyPI for platforms outside the wheel matrix; requires Rust toolchain at install time, documented as such.

---

## Phase 1: MVP — Core Symbolic Algebra (Weeks 3-22, ~20 weeks)

### Objectives
- Establish the Python/Rust split and the PyO3 boundary conventions.
- Implement foundational symbolic computation with the kernel in Rust and the API surface in Python.
- Ship a usable CLI and a `pip install`-able library.
- Establish the plugin entry-point system so it can be exercised in Phase 1.

### In Scope

#### 1.1 Expression Representation (Rust kernel)
- Hash-consed DAG of expression nodes (per §0.2).
- Atoms: arbitrary-precision integers (`num-bigint` internally, Python `int` at the boundary), rationals, IEEE-754 floats, symbols, strings.
- Operators: `+`, `-`, `*`, `/`, `^`, `=`.
- Functions: `sin`, `cos`, `tan`, `exp`, `log`, `sqrt`, `abs`.
- Lists/sequences for multi-valued results.
- Python wrapper class `monomix.Expr` with `__add__`/`__mul__`/etc. so expressions compose with normal Python operator syntax.
- **Note on numerics:** Symbolic rationals are the default representation; floats are produced only on explicit numeric evaluation (see §1.7). The two are not silently mixed.
- **Limitation:** No complex numbers (real algebra only). See §1.6 for how `solve` handles equations with no real solution.

#### 1.2 Parser (Rust kernel)
- The Phase 1 grammar subset defined in §0.6.
- Produces an AST with byte-accurate source spans on every node.
- Built-in function recognition for: `df`, `int` (Phase 2 stub raises `UnsupportedError`), `solve`, `factor` (Phase 2 stub), `expand`, `simplify`, `sub`.
- Error recovery so a single syntax error doesn't abort multi-statement input.
- Exposed to Python as `monomix.parse(source: str) -> Expr`.

#### 1.3 Variable Bindings (Python session state)
- `:=` assignment binds a symbol to an expression in the current REPL session.
- `clear x;` removes a binding.
- Bindings live in a `Session` object on the Python side (the kernel is stateless).
- **Why this is in MVP:** Without bindings, the REPL is a calculator, not a symbolic-exploration tool. Real REDUCE workflows are stateful.

#### 1.4 Symbolic Differentiation (Calculus)
- `df(f, x)` in the kernel.
- Rules for: constants, variables, sums, products, quotients, power rule, chain rule.
- Standard functions: `sin`, `cos`, `tan`, `exp`, `log`, `sqrt`, `asin`, `acos`, `atan`.
- Partial derivatives via repeated application.
- **Limitation:** No automatic simplification post-differentiation (user must call `simplify`).

#### 1.5 Polynomial Manipulation (Univariate)
- Sparse univariate polynomial representation in the kernel.
- Operations: addition, subtraction, multiplication, exponentiation, division-with-remainder.
- `expand()` and `collect()`.
- Degree and coefficient extraction.
- **Why polynomial division is in MVP:** The simplifier needs it to cancel common factors in rational expressions (`x²/x → x`). Excluding it would force `simplify` to handwave on basic cases.
- **Limitation:** No factorization (Phase 2). No multivariate polynomials (Phase 2). No Groebner basis (Phase 3+).

#### 1.6 Equation Solving
- Linear equations: `a*x + b = 0` → `x = -b/a`.
- Quadratic equations via the quadratic formula.
- General `n×n` linear systems via Gaussian elimination with partial pivoting.
- **Behavior on no real solutions:** Returns the symbolic empty-set value `{}` and emits a Python warning that complex roots exist but are not representable in MVP. Example: `solve(x² + 1 = 0)` → `{}` with `MonomixWarning("no real solutions; complex roots not supported until Phase 3")`.
- **Limitation:** No cubic or quartic general solvers. No transcendental equation solvers — `solve` raises `UnsupportedError("equation form not supported")` with a clear message.

#### 1.7 Expression Simplification
- Like-term collection: `x + x + 1` → `2*x + 1`.
- Common-factor cancellation (uses §1.5 polynomial division): `x²/x` → `x`.
- Trivial identity elimination: `0 + x → x`, `1 * x → x`, `x^1 → x`, `x^0 → 1`.
- Power rules: `x^a * x^b → x^(a+b)`, `(x^a)^b → x^(a*b)`.
- Pythagorean identity: `sin²(x) + cos²(x) → 1`, including when the terms appear inside a larger sum.
- **Pattern matching:** A minimal term-rewriting engine ships in the Rust kernel in Phase 1 to support the trig identity above. It is intentionally narrow — not a general rule database. Promotion to a full pattern-matching system is in Phase 2 §2.6.
- **Limitation:** No advanced simplification (algebraic number theory). No context-aware assumptions (`assume(x > 0)`).

#### 1.8 Substitution & Numeric Evaluation
- `sub(x = 5, expr)` (REDUCE-style syntax) and `Expr.subs({x: 5})` (Pythonic alternative).
- Numeric evaluation: explicit `evaluate_numeric(expr)` walks the tree and produces a Python `float`. Symbols without bindings raise `EvalError`. This is the only path that mixes symbolic and floating-point representations.
- **Limitation:** No lazy evaluation or delayed substitution.

#### 1.9 CLI / REPL (Python)
- Distributed as the `monomix` console script (entry point in `pyproject.toml`).
- Built on `prompt_toolkit` for line editing, history, and multi-line input; `rich` for formatted output.
- Commands: `df`, `solve`, `expand`, `simplify`, `sub`, `evaluate_numeric`, `clear`, `help`, `quit`.
- Multi-line input continues until `;` or `$`.
- Command history persisted to `~/.monomix_history`.
- `help command` produces inline documentation.
- **Limitation:** No graphing, plotting, or visualization.

#### 1.10 Plugin System (Python entry points)
- Plugins register via the `monomix.plugins` entry-point group.
- A plugin can:
  - Register a new function (e.g., a specialized `bessel_j`).
  - Register a rewrite rule that the simplifier will consider.
  - Register a pretty-printer for a custom type.
- Plugin discovery happens at `Session` construction; explicit opt-out via `Session(load_plugins=False)`.
- **Why this is in Phase 1, not deferred:** the plugin contract shapes the public API. Designing it after the API has shipped guarantees breakage. Even if zero third-party plugins exist in Phase 1, the contract is exercised by an internal "stdlib" plugin (the standard function library is itself a plugin).

#### 1.11 Distribution
- `pip install monomix` and `uv add monomix` both work.
- Wheels for the §0.9 platform matrix.
- `sdist` published to PyPI as a fallback.
- Installation tested in CI on a clean machine for each platform.

#### 1.12 Testing & Documentation
- Test strategy follows §0.7 (Rust unit + `proptest` + Python `pytest` + `hypothesis` + golden corpus).
- Coverage targets: Rust kernel >85% line coverage (`cargo llvm-cov`); Python layer >90% (`coverage.py`). The Python target is higher because Python code is mostly glue and CLI logic — uncovered glue is suspicious.
- Property-based invariants include at minimum: `simplify` idempotence, `expand`/`simplify` round-trip, `df` linearity.
- Documentation:
  - `rustdoc` for the kernel internals.
  - Sphinx + `myst-parser` for the public Python API and user guide, hosted on Read the Docs.
  - Doctests (`pytest --doctest-modules`) on every public Python function.
  - `CONTRIBUTING.md` covering both Rust and Python development workflows.

### Out of Scope

| Feature | Why Not in Phase 1 | Moved to |
|---------|-------------------|----------|
| MCP server | Doubles the surface area before the core engine is proven | Phase 1.5 |
| Integration (symbolic antiderivatives) | Complex algorithm, lower priority than differentiation | Phase 2 |
| Polynomial factorization | Requires Berlekamp / Cantor-Zassenhaus | Phase 2 |
| Matrix/linear algebra | Needs full 2D array support and ops | Phase 2 |
| Series expansion (Taylor, Laurent) | Deferred to analytical functions phase | Phase 2 |
| Limits and infinity | Requires order-of-magnitude reasoning | Phase 3 |
| Groebner basis computation | Advanced algebraic geometry, niche use case | Phase 3+ |
| Tensor algebra | Indexed objects and Einstein notation | Phase 3+ |
| Modular arithmetic | Specialized domain | Phase 3+ |
| General rule-based pattern matching | Architectural overhead beyond MVP needs | Phase 2 |
| User-defined procedures | Script execution, parameter binding, scope rules | Phase 2 |
| Module system (`load`, `include`) | Filesystem integration | Phase 2 |
| Complex numbers | Out of MVP scope (real algebra only) | Phase 3 |
| Automatic assumptions (`assume(x > 0)`) | Constraint reasoning too heavy | Phase 2+ |
| Lisp interpreter | REDUCE syntax only; avoid language-runtime complexity | Never (by design) |

---

## Phase 1.5: MCP Server (Weeks 23-28, ~6 weeks)

### Rationale for splitting MCP from Phase 1
Building the MCP server alongside the core engine doubles the debugging surface and risks shipping a half-finished engine wrapped in a half-finished protocol layer. Phase 1.5 lets the engine harden first, then exposes it cleanly.

### In Scope
- MCP server implemented in Python using the official `mcp` Python SDK.
- Synchronous request/response (no streaming yet).
- Distribution: `monomix[mcp]` extra installs the MCP dependencies; `monomix-mcp` console script launches the server.
- Tools exposed:
  - `differentiate(expression, variable)`
  - `solve_equation(equation, variable)`
  - `simplify_expression(expression)`
  - `expand_expression(expression)`
  - `substitute(expression, variable, value)`
  - `evaluate_numeric(expression)`
  - `help(command)`
- Error contract: Python exceptions map cleanly to MCP error responses with structured `code`/`message`/`data`.
- Concurrency: the Rust kernel releases the GIL on long operations (per §0.5), so concurrent MCP requests are not serialized on the Python side. Verified by load test.
- **Limitation:** No streaming responses (Phase 2). No result caching across requests (Phase 2).

### Success Criteria
- All Phase 1 CLI behavior reachable via MCP with identical results (verified by a parity test suite).
- Sustained 50 req/s on a 4-core developer machine on the Phase 1 benchmark suite.
- Conformance against the MCP Python SDK's reference test harness.

---

## Phase 2: Extended Algebra (Weeks 29-48, ~20 weeks)

### Objectives
- Add symbolic integration and advanced calculus.
- Implement matrix and linear algebra operations.
- Add streaming and caching to the MCP server.
- Support REDUCE-script loading and user-defined procedures.
- Generalize the simplifier's rewriting engine.

### In Scope

#### 2.1 Integration (Symbolic Antiderivatives)
- Integration by substitution.
- Integration by parts.
- Partial fraction decomposition.
- Standard function library (trig, exponential, logarithmic).
- Definite integrals via numerical Gaussian quadrature when no closed form exists.
- **Limitation:** No advanced techniques (residue theorem, contour integration).

#### 2.2 Polynomial Factorization
- Univariate factorization over the integers.
- Square-free factorization.
- Berlekamp's algorithm for factoring mod p, lifted to ℤ via Hensel.
- **Limitation:** No factorization over algebraic extensions. No multivariate factorization (Phase 3).

#### 2.3 Matrix Operations
- Matrix representation in the kernel; Python wrapper `monomix.Matrix`.
- Addition, subtraction, multiplication, transpose.
- Determinant via Gaussian elimination.
- Matrix inverse via Gauss-Jordan.
- Trace, rank, numerical eigenvalue approximation.
- **Limitation:** No symbolic eigenvalues/eigenvectors. No SVD or numerical-stability guarantees. No sparse-matrix optimizations.

#### 2.4 Series Expansion
- Taylor series: `taylor(f, x, a, n)`.
- Laurent series for simple poles.
- **Limitation:** No singularity classification or branch-cut tracking.

#### 2.5 Multivariate Polynomials
- Multivariate sparse representation in the kernel.
- Multivariate `expand`, `collect`, division-with-remainder under a fixed monomial order (lex by default).

#### 2.6 General Pattern Matching & Advanced Simplification
- Term-rewriting engine generalized from the §1.7 minimal version.
- User-extensible rule database via the plugin system (read-only in Phase 2; user-writable rules in Phase 3+).
- Trigonometric identity reduction beyond the Pythagorean identity.
- Rational-function simplification.
- Logarithm/exponential simplification.
- **Limitation:** No heuristic simplification ordering (no Risch-style normalization).

#### 2.7 Script Loading & User Procedures
- `load "filename.red"` executes a REDUCE script file (parsed by the Phase 1 parser, extended for the Phase 2 grammar additions below).
- User-defined procedures in REDUCE syntax: `procedure f(x, y); x^2 + y; end;`.
- `for`, `while`, `do` loops.
- Local scope and parameter passing.
- Distinction from the §1.10 plugin system: REDUCE scripts are user-authored algebra; Python entry-point plugins are package-distributed extensions to the engine itself. Both ship.
- **Limitation:** No closures or first-class functions. No macro system.

#### 2.8 Full MCP Server
- Async streaming MCP responses for long-running operations.
- All Phase 1.5 tools plus:
  - `integrate(expression, variable, [bounds])`
  - `factor_polynomial(polynomial, variable)`
  - `matrix_operations(...)` family
  - `taylor_series(expression, variable, point, order)`
  - `simplify_advanced(expression, [options])`
- Result caching with configurable eviction.
- **Limitation:** No distributed computation. No persistent caching across server restarts.

#### 2.9 Module System
- `load` and `include` directives for REDUCE-script reuse.
- Namespace isolation across loaded scripts.
- Standard-library packaging (the standard function library distributed as REDUCE scripts where appropriate).

### Out of Scope

| Feature | Why Not in Phase 2 | Moved to |
|---------|-------------------|----------|
| Groebner basis & polynomial ideals | Advanced polynomial algebra | Phase 3 |
| Multivariate polynomial factorization | Univariate-first | Phase 3 |
| Tensor algebra (Einstein notation) | Indexed objects too heavy | Phase 3 |
| Limits at infinity | Asymptotic analysis, niche | Phase 3+ |
| Complex number algebra | Real algebra still primary | Phase 3 |
| Constraint solving (CLP) | Separate paradigm | Phase 3+ |
| Numerical ODE solvers | Separate domain from symbolic | Phase 3+ |
| Graph/network algorithms | Out of CAS scope entirely | Never |

---

## Phase 3+: Specialized & Advanced Features

These are candidate features, not committed work. Prioritization happens after Phase 2 ships, based on usage signal and demand.

### Candidate Features

#### 3.1 Complex Number Support
Gaussian integers and complex-field operations; complex analysis (residues, contour integration). **Complexity:** Medium. **Estimate:** 4-6 weeks. **Note:** Likely the first Phase 3+ work because it unblocks `solve` for irreducible quadratics and integration over the complex plane.

#### 3.2 Groebner Basis & Multivariate Algebra
Buchberger's algorithm; multivariate polynomial factorization; ideal-membership testing. **Complexity:** Very high. **Estimate:** 8-12 weeks.

#### 3.3 Tensor Algebra
Indexed objects (Einstein summation), contraction, outer products, covariant/contravariant components. **Complexity:** Very high. **Estimate:** 10-14 weeks.

#### 3.4 Limits and Asymptotic Analysis
`lim(x→a) f(x)`; big-O / little-o / Θ; asymptotic expansion. **Complexity:** High. **Estimate:** 6-8 weeks.

#### 3.5 Modular Arithmetic
Finite-field operations, CRT, discrete log. **Complexity:** Medium. **Estimate:** 4-6 weeks.

#### 3.6 Constraint Solving
Linear constraint programming. **Complexity:** Very high. **Estimate:** 12+ weeks; likely a standalone package.

#### 3.7 Numerical ODE Solvers
Runge-Kutta, symplectic integrators. **Complexity:** Medium. **Estimate:** 6-8 weeks; consider a separate package.

#### 3.8 Native Decision Procedures
SMT-style reasoning over monomix's own theories — satisfiability, `prove` / `decide` / `assume`, and simplification under assumptions — implemented natively in the Rust kernel (no external solver; distinct from the user-facing constraint programming in §3.6). Tiered: (1) linear real arithmetic, (2) univariate nonlinear sign reasoning, (3) linear integer arithmetic, (4) multivariate nonlinear real (CAD / nlsat) as an open-ended frontier. Tiers 1–2 may pull forward to support the Phase 2 assumption store and piecewise simplifier. **Complexity:** High to very high (tier 4 is research-grade). **Note:** Direction set by [ADR-0004](decisions/0004-native-decision-procedures.md), superseding ADR-0003; phasing pinned by `designs/decision-procedures.md` (forthcoming).

### Process for Phase 3+ Decisions
1. **Demand check:** Concrete user request or research priority documented.
2. **Scope check:** Feature fits the CAS domain rather than expanding it.
3. **Complexity check:** Implementation is realistic in the estimated window.
4. **Architecture check:** No major refactoring of Phase 1/2 internals required.
5. **Decide & document:** Maintainer decision recorded as an ADR in `decisions/`.

---

## Explicitly Out of Scope (By Design)

These will **never** be part of this project, even after all phases:

| Feature | Reason |
|---------|--------|
| **Lisp interpreter** | REDUCE syntax only; no language runtime |
| **Graphing/plotting** | Use external tools (matplotlib, plotly) via integration if needed |
| **Statistics/probability** | Use NumPy/SciPy/R |
| **Constraint logic programming** | Requires Prolog-like execution model |
| **General theorem proving / program verification** | Out of scope. Monomix has *native* decision procedures for its own algebraic subproblems ([ADR-0004](decisions/0004-native-decision-procedures.md), §3.8), but is not a general verifier or an external-SMT frontend |
| **GPU acceleration** | Out of scope unless a clear algebraic use case emerges |
| **Distributed computation** | Single-node architecture by design |
| **Natural language input** | Parsing math from English is a separate project |

---

## Scope Governance

### Adding Features Mid-Phase
- **Red flag:** New features proposed while the current phase is <50% complete.
- **Process:** Defer to the next phase unless it's a correctness or security fix.
- **Exception:** Correctness bugs in core algebra are always fixed immediately.

### Removing Features
- If a feature exceeds 2× its estimate, it gets re-scoped to a later phase. Decision recorded as an ADR.

### Phase Transitions
- **Phase 1 → 1.5:** v0.1.0 ships with passing tests, coverage targets met, documented limitations, wheels published to PyPI, and a benchmark baseline.
- **Phase 1.5 → 2:** v0.2.0 ships with the MCP server passing conformance tests.
- **Phase 2 → 3+:** Evaluate usage signal; pick the first Phase 3+ candidate.

---

## Success Criteria by Phase

### Phase 1 (MVP)
- All §0.7 test layers green (Rust unit, Rust `proptest`, Python `pytest` + `hypothesis`, golden corpus).
- Coverage: Rust >85%, Python >90%.
- Differentiation correct on a curated 50-example textbook suite (committed to `tests/textbook/`).
- Parser handles the §0.6 grammar subset without panics on any input (verified by `cargo-fuzz` ≥1 hour).
- CLI REPL functional end-to-end.
- Wheels build and install cleanly on every platform in §0.9.
- One internal "stdlib" plugin demonstrates the §1.10 plugin contract.
- Concrete benchmark targets (measured with `criterion` for the Rust kernel and `pytest-benchmark` for the Python boundary):
  - `df` of a 20-term univariate polynomial: <50 ms wall-clock from Python.
  - `simplify` on a sum of 50 terms: <100 ms.
  - `solve` on a quadratic: <10 ms.
  - PyO3 boundary overhead per call: <500 ns.
- `cargo audit`, `cargo deny check`, and `pip-audit` all clean.
- Sphinx documentation published to Read the Docs.

### Phase 1.5 (MCP)
- All Phase 1 functionality reachable via MCP with identical results (parity test suite).
- Sustained 50 req/s on a 4-core machine on the Phase 1 benchmark suite.
- Conformance against the MCP Python SDK reference test harness.

### Phase 2
- Integration correct on a curated 30-example suite.
- Matrix operations verified against `numpy`/`sympy` on numerical inputs.
- REDUCE-script loading and user procedures work for a 10-example script corpus.
- MCP server: 100 concurrent requests sustained; result caching delivers ≥2× speedup on repeated queries.
- All Phase 1 tests still pass (no regressions).

### Phase 3+ (Per-Feature)
- Defined when the feature is committed to.

---

## References

- **REDUCE documentation:** https://reduce-algebra.sourceforge.io/docs/
- **Lisp original:** Written 1968-present in Lisp. The rewrite is clean-room — no Lisp source is read for implementation. **However**, the legacy test corpus at `legacy/reduce-algebra-code-r7357-trunk/packages/*/{*.tst,*.rlg}` is used as a correctness oracle (input/expected-output pairs), and the `.tex` documentation in the legacy tree is read for algorithmic background where helpful.
- **MCP specification:** https://modelcontextprotocol.io/
- **MCP Python SDK:** https://github.com/modelcontextprotocol/python-sdk
- **PyO3:** https://pyo3.rs/
- **Maturin:** https://www.maturin.rs/
- **Implementation language ADR:** `decisions/0001-implementation-language.md`

---

## Questions & Clarifications

### Q: Why Python on the surface and Rust underneath?
**A:** See `decisions/0001-implementation-language.md`. Short answer: Python wins on installation, AI ecosystem integration, and plugin authoring; Rust wins on raw performance for tree manipulation. The hybrid keeps each language where it pays.

### Q: Why not include complex numbers in Phase 1?
**A:** Real algebra is the 80/20 case. Complex support adds representation, simplification rules, and function-domain complexity. It's the first candidate for Phase 3+ specifically because integration and `solve` benefit most from it.

### Q: Why no automatic simplification after differentiation?
**A:** Two reasons: simplification heuristics are open-ended, and user control over when simplification happens is valuable. Power users often want unsimplified derivatives.

### Q: Why no Lisp interpreter?
**A:** The original REDUCE is Lisp-based, but a fresh implementation gains from dropping Lisp entirely. We support REDUCE syntax (the user-facing DSL) without implementing a Lisp runtime.

### Q: Why is MCP its own phase?
**A:** Earlier drafts bundled MCP into Phase 1. Building the protocol layer alongside an unproven engine doubles the debugging surface. Phase 1.5 lets the engine stabilize first, then wraps it cleanly.

### Q: Why are plugins in Phase 1 instead of later?
**A:** The plugin contract shapes the public Python API. Designing it after the API has shipped guarantees breakage. The Phase 1 work is mostly contract design — even with zero third-party plugins, the standard function library is itself a plugin, exercising the contract.

### Q: When will [feature X] be available?
**A:** Open an issue. If demand is clear, it's evaluated for Phase 2 or Phase 3.

### Q: Can I contribute a feature not on this roadmap?
**A:** Open an issue first to discuss. PRs that expand scope beyond the planned phases will be deferred to a future phase to keep the timeline honest.

---

## Version History

| Date | Version | Changes |
|------|---------|---------|
| 2026-04-26 | 1.0 | Initial scope definition for Phases 1-3+ |
| 2026-04-26 | 1.1 | Added Phase 0 foundational decisions; split MCP into Phase 1.5; resolved numerics, simplifier, and linear-system inconsistencies; corrected REDUCE syntax for user procedures; replaced "team" language with solo-maintainer model; replaced unfalsifiable success criteria with measurable benchmarks; added legacy `.tst`/`.rlg` corpus as correctness oracle; added variable bindings to MVP CLI |
| 2026-04-26 | 1.2 | Switched primary stack to Python with a Rust extension kernel (PyO3 + maturin) per ADR 0001; added §0.1 language decision, §0.8 tooling baseline, §0.9 distribution matrix; added §1.10 plugin system to Phase 1; specified Python MCP server for Phase 1.5; updated benchmarks to include PyO3 boundary overhead; added Python-specific tooling (uv, ruff, pyright, pytest, hypothesis); added Sphinx/Read the Docs as the public documentation target |
| 2026-04-26 | 1.3 | Renamed project to **Monomix**; package name `monomix` (`pip install monomix`); Python module `monomix`; CLI `monomix`; MCP server `monomix-mcp`; plugin entry-point group `monomix.plugins`; exception hierarchy rooted at `MonomixError`; environment variable `MONOMIX_PURE_PYTHON`; references to "REDUCE" preserved where they mean the original system or its syntax |
