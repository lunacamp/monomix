# Python bindings

The `monomix` package exposes the Rust kernel through PyO3. The
user-facing types are `monomix.Expr` and `monomix.Session`.

## Quick start

```python
from monomix import Session, simplify, df

s = Session()
x = s.symbol("x")
expr = x ** s.integer(3)
print(simplify(df(expr, x)))   # 3*x^2
```

## Session

A `Session` owns the underlying expression pool. Every `Expr` produced
from a session keeps the pool alive, so the `Expr` outlives the
session:

```python
def make():
    s = Session()
    return s.symbol("x")     # still usable after make() returns
```

Mixing `Expr` from two different `Session`s raises `CrossSessionError`.

## Operator surface

| Operator | Builds |
|----------|--------|
| `+ - * / ** -` | arithmetic node |
| `==` `!=` | `Eq`, `Not(Eq(...))` |
| `<` `<=` `>` `>=` | `Lt`, `Le`, `Gt`, `Ge` |
| `& \| ~` | `And`, `Or`, `Not` |

### `==` returns an expression, not a bool

```python
e = (x == 0)          # Eq(x, 0), an Expr
bool(e)               # False (handle equality on x and 0)
hash(e)               # hashable
```

For any non-`Eq` expression, `bool(...)` raises `TypeError`. Use
`e.is_same(other)` for guaranteed-bool handle equality.

### Operator precedence trap

Python's `&` and `|` bind tighter than `==`. Parenthesize:

```python
bad = a == b & c == d       # parses as a == (b & c) == d
good = (a == b) & (c == d)  # what you wanted
```

## Module-level kernel functions

| Function | Purpose |
|----------|---------|
| `simplify(e)` | run the kernel simplifier |
| `df(e, x)` | differentiate `e` with respect to symbol `x` |
| `expand(e)` | distribute products over sums |
| `solve(eq, x)` | solve `eq` for `x`; returns a list of value `Expr`s |
| `sub(mapping, e)` | substitute `{symbol: value}` simultaneously |
| `evaluate_numeric(e)` | reduce to an `f64`; raises on unbound symbols |

All kernel calls release the GIL while the Rust side runs, so two
sessions can be simplified in parallel from two Python threads.

## Errors

| Exception | When |
|-----------|------|
| `ParseError` | parser failure |
| `EvalError` | unbound symbol, division by zero, overflow |
| `UnsupportedError` | feature not in Phase 1 (e.g. `df` of a `Lt`) |
| `CrossSessionError` | mixing `Expr` from different `Session`s |
| `MonomixError` | base class for all of the above |
