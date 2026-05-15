# SMT Bridge — System Design

**Component:** `monomix.smt` (Python)
**Status:** Phase 1 — protocol only; no backend shipped in source
**Date:** 2026-05-14
**References:** SCOPE.md §1.7; ADR-0003 (SMT as the satisfiability backend); `designs/expression-dag.md`; `designs/equation-solving.md`; `docs/python-bindings.md`

---

## 1. Requirements

### 1.1 Functional requirements

The SMT bridge converts a monomix `Expr` into a term in some external satisfiability solver and exposes the solver's `prove` / `decide` / `assume` verbs to the rest of the CAS.

The required surface, in Phase 1:

- A **`Backend` protocol** that any concrete solver adapter must implement. The protocol is sort-aware (real / int / bool), arithmetic-aware (add, mul, neg, div, integer powers), comparison-aware, boolean-aware, and supports an "uninterpreted function" escape hatch for unrecognised `Fn` nodes.
- A **`Translator`** that walks an `Expr` via the inspection API on the PyO3 boundary (`kind`, `children`, `as_int`, `as_rational`, `as_float`, `as_bool`, `symbol_name`, `fn_name`) and emits backend terms via the protocol.
- A **result-type vocabulary** — `Proved`, `Refuted(counterexample)`, `Sat(model)`, `Unsat`, `Unknown` — that is solver-agnostic and that callers pattern-match against.
- A **session-aware sort registry** — symbol sorts come from `monomix.Session.sort_of(name)`, declared via `Session.declare(name, sort)`. The Translator caches `(name, sort) → backend term` lookups so the same symbol declared once is reused across translations.

The bridge is consumed by:

- The **assumption store** and **piecewise simplifier** for "is this term definitely zero / positive / between bounds" checks.
- The **CAS kernel** for branch reasoning where the symbolic engine returns `Unknown` and a solver is the next line of attack.

### 1.2 Non-functional requirements

- **No shipped backend.** `python/monomix/smt/` imports only the standard library, `monomix.Expr`, and `monomix.Session`. No solver-specific code lives in source. Users who want satisfiability checks supply their own `Backend` implementation, typically as a small adapter around an external solver in their own project or in an `extras_require` of their own package.
- **Solver-agnostic vocabulary.** No type, function, or error message in the source mentions a specific solver. The names refer to abstract concepts (`Sat`, `Backend`, `TranslationError`) so swapping or stacking backends doesn't fork the API.
- **Single-pass translation.** `Translator.to_backend(e)` walks `e` once. No re-entry into the kernel during translation; no fixed-point loop.
- **Bounded surface area.** The Backend protocol is small (~20 methods) and stable — adding capabilities requires extending the protocol, not editing every call site.
- **No panics.** Every error path raises `TranslationError`, `Unsupported`, `BackendUnavailable`, or `SolverError` (a small hierarchy under `MonomixError` via `monomix.errors`). The bridge never propagates a raw exception from a backend without wrapping it.

### 1.3 Constraints

- **Python-only.** The bridge is a thin Python layer over the PyO3 inspection API on `Expr`. There is no Rust-side counterpart and there will not be one — the kernel stays solver-free, and translation is cheap enough that no GIL release is needed.
- **Linear/nonlinear arithmetic, no quantifiers in Phase 1.** Backends are expected to handle `QF_LRA`, `QF_LIA`, and at minimum the nonlinear fragments needed for the polynomial features of the kernel (`QF_NRA` / `QF_NIA`). Quantifier translation (`∀`, `∃`) is deferred to Phase 2.
- **Sorts are name-keyed at the Session.** A `Session` stores `name → sort` mappings; a `Symbol` `Expr` is looked up by its name when the Translator first encounters it. There is no `Expr`-level sort annotation in Phase 1.
- **No model→Expr round-trip.** Backends return concrete Python values (`int`, `Fraction`, `bool`) in models; reconstructing an `Expr` from a model value is deferred. See §5.

### 1.4 What this component is **not**

- **It is not a solver.** It owns no solver state, no clause database, no search algorithm. The Backend protocol is the integration seam.
- **It is not a translator from solver back to `Expr`.** The reverse direction (model → `Expr`) is out of scope for Phase 1.
- **It is not coupled to any particular solver.** Z3 is the **parity reference** (§4) — the feature list that any conforming backend must support is derived from Z3's capability — but Z3 is not imported by source, mentioned in source identifiers, or shipped as part of the bridge.

---

## 2. High-Level Design

### 2.1 Public API

```python
# python/monomix/smt/__init__.py — public surface

from monomix.smt.translate import Backend, Translator
from monomix.smt.errors import (
    BackendUnavailable, SolverError, TranslationError, Unsupported,
)
from monomix.smt.results import (
    DecideResult, ProveResult, Proved, Refuted, Sat, Unknown, Unsat,
)
```

`Backend` is a `typing.Protocol`. `Translator(backend, session)` is the only entry point; callers wire it themselves around their chosen backend.

### 2.2 Component diagram

```
        monomix.Expr          monomix.Session
              │                       │
              ▼                       ▼
        ┌────────────────────────────────────┐
        │ Translator                         │
        │  - dispatches on Expr.kind         │
        │  - caches (name, sort) → backend   │
        │  - emits via Backend protocol      │
        └─────────────────┬──────────────────┘
                          │ Backend methods
                          ▼
                ┌───────────────────┐
                │ user-supplied     │
                │ Backend impl      │
                │ (adapter around   │
                │  an SMT solver)   │
                └────────┬──────────┘
                         │
                         ▼
                  external solver
                  (process / FFI)
```

### 2.3 Module layout

```
python/monomix/smt/
├── __init__.py        — re-exports the public API (Backend, Translator,
│                        result types, error types)
├── translate.py       — Backend protocol + Translator implementation
├── results.py         — Proved / Refuted / Sat / Unsat / Unknown dataclasses
├── errors.py          — TranslationError, Unsupported, BackendUnavailable,
│                        SolverError
└── (no backend files)
```

There are no backend implementations in the package. The `tests/` directory contains no backend-specific tests either — protocol-level translation is the only thing the source can test without a solver, and that surface is exercised via the existing `Expr` inspection tests.

### 2.4 Dispatch table

The `Translator` is a single method (`to_backend`) that pattern-matches on `Expr.kind`:

| `kind` | Backend call |
|--------|--------------|
| `SmallInt`, `BigInt` | `int_const(n)` |
| `Rational` | `rational_const(num, den)` |
| `Float` | `rational_const(p, q)` after `Fraction(f).limit_denominator(10**12)` |
| `Symbol` | `real(name)` / `int(name)` / `bool(name)` based on `Session.sort_of(name)` |
| `BoolConst` | `bool_const(b)` |
| `Add`, `Mul` | `add(*children)` / `mul(*children)` |
| `Neg` | `neg(child)` |
| `Div` | `div(num, den)` |
| `Pow` | `pow_int(base, n)` — exponent must be an integer Expr (§3.4) |
| `Eq` | `eq(l, r)` |
| `Lt`, `Le`, `Gt`, `Ge` | `lt` / `le` / `gt` / `ge` |
| `And`, `Or` | `and_(*children)` / `or_(*children)` |
| `Not` | `not_(child)` |
| `Implies` | `implies(a, b)` |
| `Fn(tag, args)` | `uninterpreted(name, args)` — escape hatch (§3.5) |

Anything not in the table raises `TranslationError`. Adding a new kernel `ExprNode` variant always requires either a new branch here or a documented `Unsupported` raise.

---

## 3. Deep Dive

### 3.1 The Backend protocol

```python
class Backend(Protocol):
    # Sort introduction
    def real(self, name: str) -> Any: ...
    def int(self, name: str) -> Any: ...
    def bool(self, name: str) -> Any: ...

    # Constants
    def rational_const(self, num: int, den: int) -> Any: ...
    def int_const(self, n: int) -> Any: ...
    def bool_const(self, b: bool) -> Any: ...

    # Arithmetic
    def add(self, *xs: Any) -> Any: ...
    def mul(self, *xs: Any) -> Any: ...
    def neg(self, x: Any) -> Any: ...
    def div(self, a: Any, b: Any) -> Any: ...
    def pow_int(self, base: Any, n: int) -> Any: ...

    # Comparison
    def eq(self, a: Any, b: Any) -> Any: ...
    def lt(self, a: Any, b: Any) -> Any: ...
    def le(self, a: Any, b: Any) -> Any: ...
    def gt(self, a: Any, b: Any) -> Any: ...
    def ge(self, a: Any, b: Any) -> Any: ...

    # Propositional
    def and_(self, *xs: Any) -> Any: ...
    def or_(self, *xs: Any) -> Any: ...
    def not_(self, x: Any) -> Any: ...
    def implies(self, a: Any, b: Any) -> Any: ...

    # Escape hatch
    def uninterpreted(self, name: str, args: list[Any]) -> Any: ...
```

Each method **returns a backend-native term**. The Translator never inspects the return value; it threads it back into the next call. The opaque `Any` is deliberate — different backends return different concrete types, and the bridge does not require a common runtime type.

The protocol is **structurally typed** (`Protocol`), so adapters do not need to inherit; any object with the matching methods qualifies.

### 3.2 Result types

```python
# python/monomix/smt/results.py

@dataclass
class Proved: ...

@dataclass
class Refuted:
    counterexample: dict[str, Any]   # symbol-name → concrete value

@dataclass
class Sat:
    model: dict[str, Any]            # symbol-name → concrete value

@dataclass
class Unsat: ...

@dataclass
class Unknown: ...

ProveResult = Proved | Refuted | Unknown
DecideResult = Sat   | Unsat   | Unknown
```

`Unknown` is **a first-class return value**, not an exception. Symbolic engines that consult an SMT backend need to distinguish "the backend says no" from "the backend gave up" — the first means the algebraic shortcut is wrong; the second means the algebraic path should still be attempted.

`counterexample` and `model` values are **plain Python**: `int`, `fractions.Fraction`, `bool`, or whatever the backend chose to expose. Reverse translation back to `Expr` is Phase 2.

### 3.3 Sort resolution

The Translator does *not* try to infer sorts from expression structure. It looks symbols up on the `Session`:

```python
def _declare_symbol(self, name: str) -> Any:
    sort = self.session.sort_of(name)      # defaults to "real"
    key = (name, sort)
    if key in self._symbols:
        return self._symbols[key]
    ref = {"real": self.backend.real,
           "int":  self.backend.int,
           "bool": self.backend.bool}[sort](name)
    self._symbols[key] = ref
    return ref
```

A symbol declared once is reused for every `Expr` translated by the same `Translator` instance. Two `Translator` instances are independent: re-translating the same `Expr` against a fresh translator allocates fresh backend terms.

Re-declaring a symbol at a different sort within one Session is currently a silent change of the `(name, sort)` cache key, producing two distinct backend symbols. Phase 2 may upgrade this to an error.

### 3.4 Integer exponents

```python
if kind == "Pow":
    base, exp = children
    exp_int = exp.as_int()
    if exp_int is None:
        raise Unsupported("non-integer exponents not supported")
    return self.backend.pow_int(self.to_backend(base), exp_int)
```

`Pow` with a symbolic exponent raises `Unsupported`. This is a deliberate constraint: SMT theories of nonlinear real arithmetic handle integer powers natively (via repeated multiplication), but `x^y` with `y` an unknown integer requires a quantified formula, which is Phase 2.

Backends may implement `pow_int` by repeated multiplication (simple, slow for large `n`) or by binary exponentiation (faster, more code). The protocol is silent on the algorithm.

### 3.5 Uninterpreted functions

Any `Fn(tag, args)` node — `sin(x)`, `exp(y)`, a user-defined `f(a, b)` — translates to `backend.uninterpreted(name, args)`. The bridge does not communicate the function's mathematical meaning to the backend; the result is a free function symbol that the solver treats as an opaque uninterpreted application.

This produces correct-but-weak results: `sin(x) == sin(x)` is provable (reflexivity), but `sin(0) == 0` is not (the backend doesn't know `sin` is the sine function). That weakness is intentional — Phase 1 keeps the bridge generic; Phase 2 may add a registration mechanism for "tell the backend that `sin` satisfies these axioms".

### 3.6 Cross-session safety

The Translator pairs with one `Session`. Mixing `Expr` from a different Session into the same translation call is already prevented at the operator level — building `e1 < e2` with `e1` from `Session A` and `e2` from `Session B` raises `CrossSessionError` before the Translator ever sees the result. The bridge inherits that guarantee for free; it has no additional cross-session check of its own.

### 3.7 Error vocabulary

| Error | When |
|-------|------|
| `TranslationError` | Translator cannot handle an `Expr.kind` (programming error or new variant not yet wired up) |
| `Unsupported` | Translation is intentionally rejected (`x ** y` with `y` non-integer; quantifiers in Phase 1) |
| `BackendUnavailable` | A backend's constructor fails because its underlying solver isn't installed. Backends raise this; the bridge does not. |
| `SolverError` | Backend reported a solver-internal error (parse error in the solver, native crash wrapped, etc.). Backends raise this; the bridge does not. |

All four inherit from `MonomixError` so a catch-all `except MonomixError` works at the application level.

---

## 4. Feature Requirements for a Conforming Backend

This section is the **parity contract**. Any backend adapter that satisfies these requirements can drop into the bridge without further changes to the source. The list is derived from Z3's capability — that is the reference implementation — but no Z3-specific behaviour leaks into the requirements.

### 4.1 Sort and constant features

| Feature | Required behaviour |
|---------|--------------------|
| Real symbols | `real(name)` returns a backend term whose theory matches "rational reals" (or the closest superset the backend supports). Repeated calls with the same `name` may return distinct terms; the bridge caches at the Translator. |
| Integer symbols | `int(name)` returns a backend term in the integer theory. |
| Boolean symbols | `bool(name)` returns a backend term in the propositional theory. |
| Integer constant | `int_const(n)` accepts arbitrary-precision Python `int`. Backends without bignum support raise `Unsupported` for values outside their range. |
| Rational constant | `rational_const(num, den)` accepts arbitrary-precision integers with `den > 0`. `den == 1` is permitted; the backend may use an integer term or a real-typed rational with denominator 1 at its discretion. |
| Boolean constant | `bool_const(b)` accepts a Python `bool`. |

### 4.2 Arithmetic features

| Feature | Required behaviour |
|---------|--------------------|
| n-ary addition | `add(*xs)` accepts ≥ 0 arguments. Zero-arg `add()` must return an additive identity (`0`). |
| n-ary multiplication | `mul(*xs)` accepts ≥ 0 arguments. Zero-arg `mul()` must return a multiplicative identity (`1`). |
| Negation | `neg(x)` — additive inverse. |
| Division | `div(a, b)` — real or integer division, matching the sort of `a` and `b`. Division by zero is the backend's problem; the bridge does not pre-check. |
| Integer powers | `pow_int(base, n)` for any Python `int` `n` (incl. `0` and negative). `pow_int(x, 0)` must return `1`; `pow_int(x, -k)` must return the reciprocal of `pow_int(x, k)`. |

### 4.3 Comparison features

| Feature | Required behaviour |
|---------|--------------------|
| Equality | `eq(a, b)` — structural / arithmetic equality, matching the sort of `a` and `b`. |
| Strict ordering | `lt(a, b)`, `gt(a, b)` for numeric `a, b`. |
| Non-strict ordering | `le(a, b)`, `ge(a, b)` for numeric `a, b`. |

### 4.4 Propositional features

| Feature | Required behaviour |
|---------|--------------------|
| n-ary `and_` / `or_` | Accept ≥ 0 arguments. Zero-arg `and_()` returns `true`; zero-arg `or_()` returns `false`. |
| `not_(x)` | Propositional negation. |
| `implies(a, b)` | Material implication. |

### 4.5 Uninterpreted functions

| Feature | Required behaviour |
|---------|--------------------|
| `uninterpreted(name, args)` | Returns a free function application. The function is the same across calls with the same `name`; arities are inferred from the first call (or per-name, at the backend's discretion). The bridge currently passes the kernel's `FnTag` name (`"sin"`, `"cos"`, `"sqrt"`, …) as `name`; backends must not interpret these — they are opaque labels in this protocol. |

### 4.6 Session features the bridge expects (separately, not on Backend)

A conforming setup also needs the backend's session-level operations — these aren't on the Backend protocol because their shape is solver-specific, but a complete integration must wrap them:

| Capability | Why |
|-----------|-----|
| Assume a term | The CAS adds hypotheses incrementally as it explores a branch. |
| `push` / `pop` scopes | Branching simplification adds temporary assumptions inside a scope and unwinds them on backtrack. |
| `check` / `decide` | Return `Sat` / `Unsat` / `Unknown`. The bridge expects a tristate. |
| Extract a model | Used to populate `Sat.model` / `Refuted.counterexample`. The model must expose symbol-name keys (matching the `name` passed into `real` / `int` / `bool`). |
| Timeout / resource bound | Required to keep `Unknown` from blocking the CAS indefinitely. The bridge does not configure this; it is the backend integration's responsibility. |

A simple session wrapper (in a downstream package, not in source) is typically ~40 lines of Python: build a `Translator`, expose `assume`, `push`, `pop`, `decide`, `prove`, and a model extractor.

### 4.7 Sample-shape session wrapper (illustrative; lives outside this repo)

```python
# Pseudocode — not part of monomix; a sketch of what an integrator writes.
class MySolverSession:
    def __init__(self, monomix_session):
        self._backend = MyTermBuilder()        # implements Backend
        self._solver  = my_solver_lib.Solver()
        self._tr      = Translator(self._backend, monomix_session)

    def assume(self, e): self._solver.add(self._tr.to_backend(e))
    def push(self):       self._solver.push()
    def pop(self):        self._solver.pop()

    def prove(self, claim, assumptions=()):
        self._solver.push()
        try:
            for a in assumptions:
                self._solver.add(self._tr.to_backend(a))
            self._solver.add(self._backend.not_(self._tr.to_backend(claim)))
            r = self._solver.check()
            if r.is_unsat():  return Proved()
            if r.is_sat():    return Refuted(counterexample=_model(self._solver))
            return Unknown()
        finally:
            self._solver.pop()
```

---

## 5. Scale, Limits, and Future Work

### 5.1 Phase 2: quantifier translation

`∀x. P(x)` and `∃x. P(x)` require both an `Expr` representation (kernel-side) and a new pair of Backend protocol methods (`forall(var, body)`, `exists(var, body)`). Deferred per SCOPE.md §1.7 (Phase 2 deliverable).

### 5.2 Phase 2: model → `Expr` reconstruction

`Refuted.counterexample` and `Sat.model` currently hand back `int` / `Fraction` / `bool`. Returning a `dict[str, Expr]` is straightforward but creates a coupling between the bridge and the `Session` (we need a pool to allocate into). The right shape is a helper `model_to_exprs(model: dict, session: Session) -> dict[str, Expr]` users opt into.

### 5.3 Phase 2: registered uninterpreted-function axioms

A `Backend.declare_axiom(name, axiom_expr)` method that lets a backend learn `sin(0) = 0`, `cos(0) = 1`, etc. Not Phase 1 because the kernel's `FnTag` set is small enough that callers can supply axioms manually inside `assumptions=[…]`.

### 5.4 Phase 2: more sorts

Bit-vectors, arrays, finite-element sorts. Each adds a small group of methods to the Backend protocol; backends that don't implement them raise `Unsupported`.

---

## 6. Testing Strategy

### 6.1 Source-level tests

The source ships **no backend**, so no end-to-end translation test runs against a solver in this repo. The `Translator`'s correctness is covered by the existing `Expr`-inspection tests (the inspection API is what the Translator depends on) plus a small *recording-backend* unit test pattern that any contributor can write to pin a specific translation shape:

```python
class RecordingBackend:
    def __init__(self): self.calls = []
    def __getattr__(self, name):
        def f(*a, **kw):
            self.calls.append((name, a, kw))
            return f"<{name}{a}>"
        return f

def test_add_dispatches_to_backend_add():
    s = Session(); x, y = s.symbol("x"), s.symbol("y")
    b = RecordingBackend()
    Translator(b, s).to_backend(x + y)
    assert ("add", _, _) in [(n, a, kw) for (n, a, kw) in b.calls]
```

These tests are not committed to the source today; they're documented here as the recommended pattern for anyone touching the Translator.

### 6.2 Downstream integration tests

End-to-end correctness — "does `x*x >= 0` actually get proved when a real backend is wired in" — is the responsibility of whichever package supplies the backend. That package should include a test matrix against at least:

- Linear real arithmetic — prove `(x + y) > 0 | x > 0 ∧ y > 0`.
- Nonlinear real arithmetic — prove `x*x >= 0`.
- Integer arithmetic — prove `n + n = 2*n` with `n` declared `"int"`.
- Push/pop scoping — assumptions added inside `push()` do not leak past `pop()`.
- Counterexample shape — refuted claim returns a model with the expected symbol keys.
- Unsupported exponents — `x ** y` (symbolic exponent) raises `Unsupported`.

The parity contract in §4 is the spec these tests verify against.

### 6.3 No fuzz testing

The Python translator is small enough (~150 lines) and the inspection API surface is fully enumerated by `Expr.kind`. Fuzz testing the bridge as a whole requires a backend, which is downstream. Fuzz testing the inspection API itself is covered by the kernel's `cargo-fuzz` parser/simplify targets, which exercise the same `ExprNode` variants.

---

## 7. Action Items

### Phase 1 — Code in this repo

1. [ ] `python/monomix/smt/translate.py` — `Backend` protocol + `Translator` (already implemented).
2. [ ] `python/monomix/smt/results.py` — split `Proved` / `Refuted` / `Sat` / `Unsat` / `Unknown` out of any backend file into their own module (currently they live in a backend file).
3. [ ] `python/monomix/smt/errors.py` — `TranslationError`, `Unsupported`, `BackendUnavailable`, `SolverError` (already implemented).
4. [ ] `python/monomix/smt/__init__.py` — re-export the public surface (Translator, Backend, result types, error types). Remove `open_session` and any backend-specific symbol.
5. [ ] Delete the in-repo backend implementation file; the source ships protocol + Translator only.
6. [ ] Drop the SMT-solver optional dependency from `python/pyproject.toml`; the source no longer imports any solver.

### Phase 2 — Deferred

7. [ ] Quantifier extensions (§5.1).
8. [ ] Model → `Expr` helper (§5.2).
9. [ ] Backend axiom registration (§5.3).
10. [ ] Additional sorts (§5.4).
