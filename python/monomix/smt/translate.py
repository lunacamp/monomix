"""Translate Monomix Expr (Rust-backed) into backend AST.

The translator walks a kernel ExprNode tree via the Expr inspection
API (`expr.kind`, `expr.children()`, `expr.as_int()`, etc.) and emits
backend-specific terms via a small Backend protocol.

No backend is shipped in this package; users provide an adapter that
implements the `Backend` protocol. See `designs/smt.md` for the
parity contract every backend must satisfy.
"""

from __future__ import annotations

from fractions import Fraction
from typing import Any, Protocol

from monomix import Expr, Session

from .errors import TranslationError, Unsupported


class Backend(Protocol):
    """Minimum interface a backend must provide."""

    def real(self, name: str) -> Any: ...
    def int(self, name: str) -> Any: ...
    def bool(self, name: str) -> Any: ...
    def rational_const(self, num: int, den: int) -> Any: ...
    def int_const(self, n: int) -> Any: ...
    def bool_const(self, b: bool) -> Any: ...

    def add(self, *xs: Any) -> Any: ...
    def mul(self, *xs: Any) -> Any: ...
    def neg(self, x: Any) -> Any: ...
    def div(self, a: Any, b: Any) -> Any: ...
    def pow_int(self, base: Any, n: int) -> Any: ...

    def eq(self, a: Any, b: Any) -> Any: ...
    def lt(self, a: Any, b: Any) -> Any: ...
    def le(self, a: Any, b: Any) -> Any: ...
    def gt(self, a: Any, b: Any) -> Any: ...
    def ge(self, a: Any, b: Any) -> Any: ...

    def and_(self, *xs: Any) -> Any: ...
    def or_(self, *xs: Any) -> Any: ...
    def not_(self, x: Any) -> Any: ...
    def implies(self, a: Any, b: Any) -> Any: ...

    def uninterpreted(self, name: str, args: list[Any]) -> Any: ...


class Translator:
    """Stateful translator caching backend declarations per symbol."""

    def __init__(self, backend: Backend, session: Session) -> None:
        self.backend = backend
        self.session = session
        self._symbols: dict[tuple[str, str], Any] = {}

    def to_backend(self, e: Expr) -> Any:
        kind = e.kind

        if kind == "SmallInt" or kind == "BigInt":
            n = e.as_int()
            assert n is not None
            return self.backend.int_const(n)
        if kind == "Rational":
            r = e.as_rational()
            assert r is not None
            return self.backend.rational_const(r[0], r[1])
        if kind == "Float":
            f = e.as_float()
            assert f is not None
            frac = Fraction(f).limit_denominator(10**12)
            return self.backend.rational_const(frac.numerator, frac.denominator)
        if kind == "Symbol":
            name = e.symbol_name()
            assert name is not None
            return self._declare_symbol(name)
        if kind == "BoolConst":
            b = e.as_bool()
            assert b is not None
            return self.backend.bool_const(b)

        children = e.children()

        if kind == "Add":
            return self.backend.add(*[self.to_backend(c) for c in children])
        if kind == "Mul":
            return self.backend.mul(*[self.to_backend(c) for c in children])
        if kind == "Neg":
            return self.backend.neg(self.to_backend(children[0]))
        if kind == "Div":
            return self.backend.div(
                self.to_backend(children[0]), self.to_backend(children[1])
            )
        if kind == "Pow":
            base, exp = children
            exp_int = exp.as_int()
            if exp_int is None:
                raise Unsupported("non-integer exponents not supported")
            return self.backend.pow_int(self.to_backend(base), exp_int)
        if kind == "Eq":
            return self.backend.eq(
                self.to_backend(children[0]), self.to_backend(children[1])
            )
        if kind == "Lt":
            return self.backend.lt(
                self.to_backend(children[0]), self.to_backend(children[1])
            )
        if kind == "Le":
            return self.backend.le(
                self.to_backend(children[0]), self.to_backend(children[1])
            )
        if kind == "Gt":
            return self.backend.gt(
                self.to_backend(children[0]), self.to_backend(children[1])
            )
        if kind == "Ge":
            return self.backend.ge(
                self.to_backend(children[0]), self.to_backend(children[1])
            )
        if kind == "And":
            return self.backend.and_(*[self.to_backend(c) for c in children])
        if kind == "Or":
            return self.backend.or_(*[self.to_backend(c) for c in children])
        if kind == "Not":
            return self.backend.not_(self.to_backend(children[0]))
        if kind == "Implies":
            return self.backend.implies(
                self.to_backend(children[0]), self.to_backend(children[1])
            )
        if kind == "Fn":
            name = e.fn_name()
            assert name is not None
            return self.backend.uninterpreted(
                name, [self.to_backend(c) for c in children]
            )

        raise TranslationError(f"unhandled Expr kind: {kind}")

    def _declare_symbol(self, name: str) -> Any:
        sort = self.session.sort_of(name)
        key = (name, sort)
        if key in self._symbols:
            return self._symbols[key]
        if sort == "real":
            ref = self.backend.real(name)
        elif sort == "int":
            ref = self.backend.int(name)
        elif sort == "bool":
            ref = self.backend.bool(name)
        else:
            raise TranslationError(f"unknown sort {sort!r}")
        self._symbols[key] = ref
        return ref
