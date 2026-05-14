# Python bindings — Phase 1 surface + Session

**Date:** 2026-05-13
**Status:** Draft (pending user review)
**Phase:** 1
**Scope:** First slice of Python bindings — the Phase 1 algebra surface plus `Session`. No plugin discovery, no REPL.

## 1. Goal

Wire the Rust `monomix-kernel` to Python via PyO3 + maturin, replacing the placeholder dataclass IR in `python/monomix/expr.py`. After this work, `monomix.Expr` is a Rust-backed handle, the SMT bridge consumes it directly, and the project has one canonical expression IR.

## 2. Scope

### In scope
- A new `rust/monomix-py/` crate that builds the Python extension module.
- A PyO3-backed `monomix.Expr` opaque type and `monomix.Session` pool owner.
- Module-level functions: `parse`, `simplify`, `df` (differentiate), `expand`, `solve`, `sub` (substitute), `evaluate_numeric`.
- Python operator overloading on `Expr` for arithmetic, comparisons, and boolean ops.
- Kernel `ExprNode` extensions for comparison and propositional nodes.
- Exception hierarchy rooted at `MonomixError`.
- GIL release on long kernel calls.
- Rewrite of the existing SMT bridge (`python/monomix/solver/` → `python/monomix/smt/`) to consume the new Rust-backed `Expr`.
- Hand-written `.pyi` stub for pyright strict.
- Minimal docs: one markdown page, doctests on public functions.

### Out of scope (deferred, called out explicitly)
- Plugin entry-point discovery (Phase 1 §1.10).
- CLI / REPL (Phase 1 §1.9).
- CI wheel matrix (SCOPE §0.9 — separate spec).
- Sphinx / Read the Docs setup.
- New SMT capabilities beyond the existing bridge's feature set.
- Reverse `model → Expr` reconstruction in the SMT bridge.
- Additional SMT backends.
- REDUCE-syntax extension for inequalities / boolean operators (the new kernel variants are reachable only via Python constructors).
- The `rust/solver-bridge` crate stays untouched (currently a non-buildable Phase 2 sketch).

## 3. Architecture

### 3.1 Workspace and packaging

```
monomix/                       (repo root)
├── Cargo.toml                 (workspace: + rust/monomix-py)
├── rust/
│   ├── monomix-kernel/        (unchanged + new ExprNode arms — §4)
│   └── monomix-py/            (NEW: PyO3 bindings, single #[pymodule])
└── python/
    ├── pyproject.toml         (build-backend = maturin)
    └── monomix/
        ├── __init__.py        (public API re-exports)
        ├── _kernel.*.{so,pyd,dylib}  (built by maturin — not in git; extension is platform-specific)
        ├── _core.py           (thin Python wrappers around _kernel)
        ├── session.py         (Session class — §5)
        ├── errors.py          (MonomixError hierarchy — §6)
        ├── _kernel.pyi        (stubs for pyright strict)
        └── smt/               (renamed from solver/, rewritten — §7)
            ├── __init__.py
            ├── translate.py
            └── z3_backend.py
```

- `pyproject.toml` switches from `setuptools` to `maturin`. `[tool.maturin]` declares `python-source = "."` and `module-name = "monomix._kernel"`.
- The kernel crate stays a pure-Rust library — `cargo test` / `cargo bench` / `cargo fuzz` continue to work without Python in the loop.
- `rust/monomix-py/` produces a `cdylib`. It depends on `monomix-kernel` as a path dep.

### 3.2 Pool ownership model

The Rust kernel uses an arena-pool model: `ExprPool` owns all nodes, `ExprId` is a 32-bit handle valid only inside its own pool, and hash-consing dedupes per-pool. The simplifier's cache is keyed by `(registry_id, ExprId)` so it's pool-local too.

- Each `Session` owns one `Arc<Mutex<ExprPool>>`.
- Every `Expr` keeps an `Arc<Mutex<ExprPool>>` clone, so it stays valid past `Session.__exit__`.
- Binary ops on `Expr` check `Arc::ptr_eq(self.pool, other.pool)` before doing anything; mismatch raises `CrossSessionError`.
- There is no implicit "default Session". Module-level functions (`parse`, `simplify`, …) operate on `Expr` directly and pull the Session out of the `Expr`'s pool ref.

### 3.3 Tier layout (for reference)

| Tier | Location | Responsibility |
|------|----------|----------------|
| Python surface | `python/monomix/` | Glue, formatting, state (`Session`, bindings, error classes, printers, SMT). |
| PyO3 boundary | `rust/monomix-py/` | Type conversion, GIL management, error mapping. Single crate; thin. |
| Rust kernel | `rust/monomix-kernel/` | All symbolic computation. Stateless. |

## 4. Kernel `ExprNode` extensions

The kernel grows comparison and propositional nodes so the SMT bridge can consume a single IR.

```rust
pub enum ExprNode {
    // (existing variants — unchanged)

    // Comparison (binary)
    Lt(ExprId, ExprId),
    Le(ExprId, ExprId),
    Gt(ExprId, ExprId),
    Ge(ExprId, ExprId),

    // Propositional
    Not(ExprId),
    And(Box<[ExprId]>),
    Or(Box<[ExprId]>),
    Implies(ExprId, ExprId),
    BoolConst(bool),
}
```

### 4.1 Size budget

The `_EXPR_NODE_SIZE_GUARD` compile-time assertion stays at `<= 32 bytes`. No new variant exceeds the current 16-byte payload ceiling (`Box<[ExprId]>`). The discriminant byte is already in the layout; nine new discriminants stay well inside `u8`.

### 4.2 Touch-ups (mechanical, to keep matches exhaustive)

- `content_hash`: hash arms for the new variants.
- `subtree_size_of`: `1 + Σ children`.
- `children`: return contained `ExprId`s.
- `fold_impl`: recurse into children.
- `map_bottom_up`: rebuild via the new normalizing constructors.
- `is_atom`: `BoolConst` is an atom; the rest are composite.

### 4.3 Normalizing constructors on `ExprPool`

```rust
pub fn lt(&mut self, a: ExprId, b: ExprId) -> ExprId
pub fn le(&mut self, a: ExprId, b: ExprId) -> ExprId
pub fn gt(&mut self, a: ExprId, b: ExprId) -> ExprId
pub fn ge(&mut self, a: ExprId, b: ExprId) -> ExprId
pub fn not_node(&mut self, x: ExprId) -> ExprId          // not(not(x)) → x; not(true)/not(false) fold
pub fn and_(&mut self, children: Vec<ExprId>) -> ExprId  // flatten + sort; and(true, x) → x; and(false, _) → false
pub fn or_(&mut self, children: Vec<ExprId>) -> ExprId   // and(true, _) → true; or(false, x) → x
pub fn implies(&mut self, a: ExprId, b: ExprId) -> ExprId
pub fn bool_const(&mut self, b: bool) -> ExprId
```

Constant folding lives in the constructors (matches the pattern used by `add`/`mul`/`pow`/`neg`/`div`/`pow`).

### 4.4 Downstream behavior

- **Simplifier driver:** no rewrite rules for booleans in Phase 1. `map_bottom_up` recurses through the new arms; that's enough.
- **Differentiator:** explicit arms returning `UnsupportedError` for comparison / boolean Exprs (not `unreachable!()`).
- **Parser:** unchanged. The REDUCE-subset grammar (SCOPE §0.6) doesn't include `<`, `<=`, `&`, `|`, etc. New variants are reachable only via Python constructors.
- **`evaluate_numeric`:** returns `UnsupportedError` for comparison / boolean Exprs. Closed-form constant folding could be added later; out of scope here.

## 5. PyO3 boundary surface

### 5.1 `monomix.Expr`

`#[pyclass]` wrapping `(Arc<Mutex<ExprPool>>, ExprId)`. Operator overloads:

| Python | Builds |
|--------|--------|
| `a + b`, `a - b`, `a * b`, `a / b`, `a ** n`, `-a` | `Add`, `Add(neg)`, `Mul`, `Div`, `Pow`, `Neg` |
| `a == b`, `a != b` | `Eq(a, b)`, `Not(Eq(a, b))` |
| `a < b`, `a <= b`, `a > b`, `a >= b` | `Lt`, `Le`, `Gt`, `Ge` |
| `a & b`, `a \| b`, `~a` | `And`, `Or`, `Not` |
| `int(a)` | exact int if numeric atom else `EvalError` |
| `float(a)` | calls `evaluate_numeric` |

Reflected dunders (`__radd__`, `__rand__`, …) coerce Python literals so `1 + x` and `True & expr` work either side.

#### 5.1.1 `__bool__`

- If the root node is `Eq(a, b)` → `bool` = `a.is_same(b)` (handle equality).
- If the root node is `Not(Eq(a, b))` → `bool` = `not a.is_same(b)`.
- Any other shape → `TypeError("ambiguous truth value of symbolic expression — use is_same(...) or evaluate first")`.

This is the same pattern NumPy uses for ambiguous arrays.

#### 5.1.2 `__hash__`

Hash by `ExprId.0`. Hash-consing makes structural equality ≡ handle equality *within one pool*. Therefore the Python contract `bool(a == b) == True → hash(a) == hash(b)` holds because both reduce to handle equality.

**Invariant:** `__bool__` of `Eq(a, b)` is correct *because* hash-consing guarantees structural equality ⇒ handle equality within one pool, and cross-pool ops raise `CrossSessionError` before reaching this path. If hash-cons semantics ever change, this section must be revisited.

#### 5.1.3 `is_same(other) -> bool`

Named method for unambiguous handle equality. Use when you need a `bool` without going through `__bool__`.

#### 5.1.4 Inspection API (used by the SMT translator)

```python
expr.kind: str            # "SmallInt" | "Rational" | "Symbol" | "Add" | ... | "Lt" | "And" | ...
expr.children() -> list[Expr]
expr.as_int() -> int | None
expr.as_rational() -> tuple[int, int] | None
expr.as_float() -> float | None
expr.symbol_name() -> str | None
expr.fn_name() -> str | None       # for Fn(Custom(...)) and built-in tag names
```

### 5.2 `monomix.Session`

```python
class Session:
    def __init__(self) -> None: ...
    def parse(self, source: str) -> Expr: ...
    def symbol(self, name: str) -> Expr: ...
    def integer(self, n: int) -> Expr: ...
    def rational(self, p: int, q: int) -> Expr: ...

    # Bindings (SCOPE §1.3) — Python-side dict; kernel stays stateless
    def assign(self, name: str, value: Expr) -> None: ...    # `:=`
    def clear(self, name: str) -> None: ...
    def bindings(self) -> dict[str, Expr]: ...

    # SMT sort declarations (used by monomix.smt)
    def declare(self, name: str, sort: Literal["real", "int", "bool"]) -> None: ...

    def __enter__(self) -> Self: ...
    def __exit__(self, *exc) -> None: ...
```

`Session.parse(":=")` resolves bindings before returning the `Expr`, so downstream ops see a fully-substituted tree.

### 5.3 Module-level functions

```python
monomix.parse(src: str, *, session: Session | None = None) -> Expr
monomix.simplify(e: Expr) -> Expr
monomix.df(e: Expr, x: Expr) -> Expr             # alias: differentiate
monomix.expand(e: Expr) -> Expr
monomix.solve(eq: Expr, x: Expr) -> list[Expr]
monomix.sub(mapping: dict[Expr, Expr], e: Expr) -> Expr   # alias: substitute
monomix.evaluate_numeric(e: Expr) -> float
```

`parse` is the only function that needs a `Session` parameter (it has no `Expr` argument). Everything else pulls the Session out of the input `Expr`'s pool ref.

### 5.4 Named constructors for boolean / comparison shape

Module-level builder *functions* (not classes) for comparison/boolean shape, used when operator overloading is awkward (e.g. literal-on-the-left) or when you want to be explicit:

```python
monomix.Eq(a, b), monomix.Lt(a, b), monomix.Le(a, b), monomix.Gt(a, b), monomix.Ge(a, b)
monomix.And(*xs), monomix.Or(*xs), monomix.Not(x), monomix.Implies(a, b)
monomix.TRUE, monomix.FALSE     # BoolConst atoms (singletons per Session)
```

Each returns a new `Expr`. They take any `Expr` operands; literal coercion is the same as for the operators.

### 5.5 Documentation hazard — operator precedence

Python's `&` and `|` bind *tighter* than `==`. So `a == b & c == d` parses as `a == (b & c) == d`, not `(a == b) & (c == d)`. Document with an explicit example and recommend parenthesization. This is not enforceable at the type-system level; warn instead.

## 6. Error model and GIL

### 6.1 Exception hierarchy (`python/monomix/errors.py`)

```python
class MonomixError(Exception): ...
class ParseError(MonomixError): ...        # syntax errors, with .span = (start, end)
class EvalError(MonomixError): ...         # unbound symbol, division by zero, etc.
class UnsupportedError(MonomixError): ...  # feature not in Phase 1 (e.g. df of a Lt)
class CrossSessionError(MonomixError): ... # mixing Exprs from different pools
```

### 6.2 Mapping from `KernelError`

| `KernelError` variant | Python class | Attributes |
|------------------------|--------------|------------|
| `Parse { msg, span }` | `ParseError` | `.span = (start, end)`, `.message` |
| `UnboundSymbol(name)` | `EvalError` | `.symbol = name` |
| `DivisionByZero` | `EvalError` | — |
| `ExponentTooLarge` | `EvalError` | — |
| `Unsupported(reason)` | `UnsupportedError` | `.reason` |
| anything else | `MonomixError` | catch-all |

Implementation: `impl From<KernelError> for PyErr` in `rust/monomix-py/src/errors.rs`.

### 6.3 GIL release

PyO3 methods wrap kernel calls in `Python::allow_threads(|| ...)` for:
- `parse`, `simplify`, `differentiate`, `expand`, `solve`, `evaluate_numeric`, `substitute`.

GIL is **not** released for:
- Atom constructors (`Session.integer/rational/symbol`) — sub-1µs.
- Operator dunders — constructing a single Add/Mul/etc. is fast.

Concurrency: the pool's `Mutex` serializes work *within* a Session; cross-Session work runs in parallel. This is what enables the Phase 1.5 MCP server.

### 6.4 BigInt boundary

- **In:** PyO3's `BigInt` `FromPyObject` impl converts Python `int` → `num_bigint::BigInt` natively. Used by `Session.integer(n)` and operator-overload coercion.
- **Out:** `int(expr)` extracts `BigInt` / `SmallInt` / `Rational` and converts; non-exact Expr raises `EvalError`.
- `float(expr)` calls `evaluate_numeric` (the only path that mixes symbolic with f64).

### 6.5 Expr lifetime

`Expr` holds an `Arc<Mutex<ExprPool>>`. An `Expr` returned from a `Session` stays valid even after the `Session` is dropped (the pool drops when the last `Expr` holding it does). There is no per-`ExprId` GC inside the pool — the arena grows for the pool's lifetime.

## 7. SMT bridge rewrite

### 7.1 Naming

- Bridge package: `monomix.smt` (renamed from `monomix.solver`).
- Z3-specific backend: `monomix.smt.z3_backend`. Z3 is the parity reference and the current backend, not the bridge's identity.

### 7.2 Public API (unchanged)

```python
from monomix.smt import (
    open_session, BackendUnavailable, Unsupported,
    Sat, Unsat, Proved, Refuted,
)
```

### 7.3 Translator

`monomix/smt/translate.py` becomes a walker over kernel `ExprNode` kinds, using the inspection API from §5.1.4.

Mapping:
- Arithmetic atoms + `Add` / `Mul` / `Pow` / `Neg` / `Div` → backend Real or Int term.
- `Eq` / `Lt` / `Le` / `Gt` / `Ge` → backend Bool term.
- `And` / `Or` / `Not` / `Implies` / `BoolConst` → backend Bool term.
- `Fn(Custom(name), args)` → backend uninterpreted function (same fallback the current code has).

### 7.4 Sort discovery

The dataclass IR carried `sort: "real" | "int" | "bool"` on `Symbol`. The Rust kernel has one symbol kind, so sorts move to the Session:

- `Session.declare(name, sort)` records sort metadata.
- Defaults to `"real"` if undeclared.
- The SMT translator reads sort declarations from the Session attached to the input `Expr`.

### 7.5 Symbol identity

Translator memoizes `(kernel_symbol_name, sort) → backend constant` in a dict scoped to the SMT session. Same name on different calls → same backend variable, so accumulated assumptions reference the right thing.

### 7.6 Backend protocol

Minimum interface the translator needs from a backend: `real(name)`, `int(name)`, `bool(name)`, plus arithmetic and boolean combinators. The Z3 backend implements this; no further abstraction in this work. If a second backend ever lands, it follows the same contract.

### 7.7 Tests

`python/tests/test_smt.py` replaces `test_solver.py`. Every current case ports over with:
- `from monomix.smt import ...` instead of `from monomix.solver import ...`.
- Expression construction via operator overloads (`x + y > 0` instead of `gt(add(x, y), Rational.of(0))`).

Additions:
- Cross-session refusal: `s1.parse("x") < s2.parse("y")` raises `CrossSessionError`.
- Parse → translate → solve round-trip (the parser now produces real Rust `Expr`s, so this exercises the full chain).
- `Session.declare("n", "int")` then `s.prove(eq(add(n, n), mul(2, n)))` uses integer sort in the backend.

## 8. Testing

### 8.1 Rust kernel (existing pattern, extended)

- Unit tests for each new normalizing constructor in `expr/mod.rs`: double-negation, flatten, dedup, constant folding, `BoolConst` interning.
- `proptest`: extend the existing "round-trip through `map_bottom_up` is identity" property to include boolean trees.
- The 32-byte size guard (compile-time + runtime) already exists.

### 8.2 PyO3 boundary (`python/tests/`)

- `test_expr.py` — operator overloading, literal coercion, `__eq__` returning `Eq`, `__bool__` rules, comparison + boolean overloads, `__hash__` consistency, cross-pool ops raise `CrossSessionError`.
- `test_session.py` — pool lifetime past `__exit__`, bindings (`assign`/`clear`), `parse(":=")` resolves bindings, `declare("n", "int")` records sort.
- `test_kernel_calls.py` — happy + error path per module-level function. Verify `ParseError.span`, `EvalError.symbol`.
- `test_gil.py` — two concurrent `simplify`s on two Sessions are ~1× single-call wall time; same-Session calls are serialized. Marked soft-floor / `@pytest.mark.benchmark` to tolerate slow CI.
- `test_smt.py` — replaces `test_solver.py` (per §7.7).

### 8.3 Property tests (`hypothesis`)

Minimal in this slice:
- `simplify(parse(repr(e))) == simplify(e)` for randomly generated `e` (round-trip parser/printer).
- `df(simplify(e), x) == simplify(df(e, x))` — kernel already tests this in Rust; the Python version proves the boundary preserves it.

### 8.4 Type checking

Hand-written `python/monomix/_kernel.pyi` covers the PyO3 surface. CI runs `pyright --strict python/monomix python/tests`. Regenerate-from-Cargo isn't worth it at this size.

### 8.5 Build

- `pyproject.toml` → `build-backend = "maturin"`, `[tool.maturin] python-source = "."`, `module-name = "monomix._kernel"`.
- Dev: `maturin develop` from `python/`; then `pytest`.
- CI wheel matrix is **not** in this work — it's a Phase 1 §0.9 deliverable with its own spec. Slice-2 exit criterion is that `maturin build` succeeds locally.

## 9. Docs

- One markdown page: `docs/python-bindings.md` covers the operator surface, the `Session` model, the precedence trap with `==` and `&`, and `CrossSessionError`.
- Doctests in module-level docstrings on each public function (`monomix.parse`, `simplify`, etc.). `pytest --doctest-modules` picks them up.
- `CLAUDE.md` amendment when the work lands: flip the "only active code" paragraph; point the SMT bridge section at `monomix.smt`.
- Sphinx setup is **not** in this work.

## 10. Implementation order

Eight slices. Each ends with `cargo test && pytest` green.

| # | Slice | Touches |
|---|-------|---------|
| 1 | Kernel `ExprNode` extensions | `rust/monomix-kernel/` only |
| 2 | Workspace + walking skeleton (`monomix-py` crate, switch `pyproject.toml` to maturin, single `#[pymodule]` exposing `__version__`) | `Cargo.toml`, new `rust/monomix-py/`, `python/pyproject.toml` |
| 3 | `Session` + opaque `Expr` (no operators yet; constructors, `__repr__`, cross-session guard, pool-lifetime tests, first `.pyi`) | `rust/monomix-py/`, `python/monomix/` |
| 4 | Operator overloading on `Expr` (arithmetic, comparisons, boolean, `__hash__`, `__bool__`, reflected dunders) | `rust/monomix-py/` |
| 5 | Module-level kernel functions with error mapping + GIL release | `rust/monomix-py/`, `python/monomix/errors.py` |
| 6 | Session bindings (`assign`/`clear`/`bindings`) + `declare(name, sort)` | `python/monomix/session.py` |
| 7 | SMT bridge rewrite (`solver/` → `smt/`, translator over new `Expr`, port + extend tests) | `python/monomix/smt/`, `python/tests/test_smt.py` |
| 8 | Docs + doc-debt notes (markdown page, doctests, `CLAUDE.md` amendment, ADR-0002 / `crates/` vs `rust/` correction list as a follow-up) | `docs/`, `CLAUDE.md`, `decisions/` |

## 11. Risks

1. **Maturin first-build pain.** `maturin develop` needs a Rust toolchain in the venv. Document setup; CI wiring is its own spec.
2. **Exhaustive matches in the kernel.** Adding `ExprNode` variants will compile-error every `match` site. Grep `ExprNode::` and fix each in slice 1. Likely sites: simplifier driver, printer, span tracker, `evalnum`.
3. **`__bool__` correctness depends on hash-cons.** Stated as an invariant in §5.1.2. If hash-cons semantics ever change, revisit.
4. **`rust/solver-bridge` workspace member.** Already in the workspace, doesn't build (Z3 deps commented). Out of scope here — don't touch.
5. **`test_gil.py` is timing-sensitive.** Mark as soft-floor / benchmark so slow CI doesn't false-fail.
6. **Doc inconsistencies** surfaced during design:
   - ADR-0002 says `Arc<ExprNode>`; reality is `ExprPool` + `ExprId`.
   - ADR-0002 says `crates/monomix-kernel/`; reality is `rust/monomix-kernel/`.
   These are *acknowledged* in slice 8's follow-up list, **not fixed** in this work.

## 12. Open questions for the implementation plan

- Should `Session` be a `#[pyclass]` directly or a Python class wrapping a `_SessionHandle` `#[pyclass]`? Recommendation: Python class wrapping a handle, so bindings dict stays in Python where it's easy to introspect.
- Should `monomix.parse(src)` without a `session=` kwarg create a fresh Session, or error? Recommendation: error — explicit Session avoids accidental cross-pool ops later.
- `__repr__` vs `__str__` format. Recommendation: `__str__` produces REDUCE-like syntax (`x^2 + sin(x)`); `__repr__` is `Expr("x^2 + sin(x)")` for unambiguous round-trip. Finalize during slice 3.

These are tactical decisions for the implementation plan, not load-bearing for the spec.
