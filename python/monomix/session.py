"""Python-side Session: holds a kernel _SessionHandle plus Python-only state.

The Session owns the ExprPool (indirectly via _SessionHandle). All
mutable state — variable bindings, symbol sort declarations — lives in
the Python class; the kernel itself stays stateless.
"""

from __future__ import annotations

from typing import Literal, Self

from monomix._kernel import Expr, _SessionHandle

Sort = Literal["real", "int", "bool"]


class Session:
    """A monomix evaluation session.

    Owns an ExprPool. Every Expr produced from a Session keeps a
    reference to the underlying pool, so Exprs stay valid past the
    Session's lifetime.
    """

    def __init__(self) -> None:
        self._handle = _SessionHandle()
        self._bindings: dict[str, Expr] = {}
        self._sorts: dict[str, Sort] = {}

    # -- atom constructors -------------------------------------------------

    def symbol(self, name: str) -> Expr:
        return self._handle.symbol(name)

    def integer(self, n: int) -> Expr:
        return self._handle.integer(n)

    def rational(self, p: int, q: int) -> Expr:
        return self._handle.rational(p, q)

    def parse(self, source: str) -> Expr:
        parsed = self._handle.parse(source)
        if not self._bindings:
            return parsed
        from monomix._kernel import sub
        mapping = {self.symbol(name): value for name, value in self._bindings.items()}
        return sub(mapping, parsed)

    # -- Symbol sort declarations -----------------------------------------

    def declare(self, name: str, sort: Sort) -> None:
        if sort not in ("real", "int", "bool"):
            raise ValueError(
                f"sort must be 'real', 'int', or 'bool'; got {sort!r}"
            )
        self._sorts[name] = sort

    def sort_of(self, name: str) -> Sort:
        return self._sorts.get(name, "real")

    # -- Bindings ---------------------------------------------------------

    def assign(self, name: str, value: Expr) -> None:
        self._bindings[name] = value

    def clear(self, name: str) -> None:
        self._bindings.pop(name, None)

    def bindings(self) -> dict[str, Expr]:
        return dict(self._bindings)

    # -- context manager ---------------------------------------------------

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *exc: object) -> None:
        return None
