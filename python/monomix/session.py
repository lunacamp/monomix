"""Python-side Session: holds a kernel _SessionHandle plus Python-only state.

The Session owns the ExprPool (indirectly via _SessionHandle). All
mutable state — variable bindings, SMT sort declarations — lives in
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
        return self._handle.parse(source)

    # -- SMT sort declarations --------------------------------------------

    def declare(self, name: str, sort: Sort) -> None:
        if sort not in ("real", "int", "bool"):
            raise ValueError(
                f"sort must be 'real', 'int', or 'bool'; got {sort!r}"
            )
        self._sorts[name] = sort

    def sort_of(self, name: str) -> Sort:
        return self._sorts.get(name, "real")

    # -- context manager ---------------------------------------------------

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *exc: object) -> None:
        return None
