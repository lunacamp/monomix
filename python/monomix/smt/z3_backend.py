"""Z3 backend for the SMT bridge.

Implements the Backend protocol from `translate.py` and provides the
session interface (push/pop/assume/decide/prove/declared_symbols).

Z3 is the parity reference for what the bridge must support; other
backends would plug into the same Backend protocol.
"""
# pyright: reportOptionalMemberAccess=false
# `z3` is None only when the package is missing — every z3.* access is gated by
# _require_z3() at the top of Z3Backend.__init__, so optional-member checks here
# would flag code paths that are unreachable in practice.

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from monomix import Expr, Session as MonomixSession

from .errors import BackendUnavailable
from .translate import Translator

try:
    import z3  # type: ignore
except ImportError as e:
    z3 = None  # type: ignore[assignment]
    _IMPORT_ERROR: ImportError | None = e
else:
    _IMPORT_ERROR = None


def _require_z3():
    if z3 is None:
        raise BackendUnavailable(
            "z3-solver is not installed. Install with `pip install z3-solver`."
        ) from _IMPORT_ERROR


# ----------------------------------------------------------------------
# Result types
# ----------------------------------------------------------------------

@dataclass
class Proved: ...


@dataclass
class Refuted:
    counterexample: dict[str, Any]


@dataclass
class Sat:
    model: dict[str, Any]


@dataclass
class Unsat: ...


@dataclass
class Unknown: ...


ProveResult = Proved | Refuted | Unknown
DecideResult = Sat | Unsat | Unknown


# ----------------------------------------------------------------------
# Term builder implementing the Backend protocol
# ----------------------------------------------------------------------

class Z3TermBuilder:
    def real(self, name: str) -> Any:
        return z3.Real(name)

    def int(self, name: str) -> Any:
        return z3.Int(name)

    def bool(self, name: str) -> Any:
        return z3.Bool(name)

    def rational_const(self, num: int, den: int) -> Any:
        if den == 1:
            return z3.RealVal(num)
        return z3.Q(num, den)

    def int_const(self, n: int) -> Any:
        return z3.IntVal(n)

    def bool_const(self, b: bool) -> Any:
        return z3.BoolVal(b)

    def add(self, *xs: Any) -> Any:
        if not xs:
            return z3.RealVal(0)
        out = xs[0]
        for x in xs[1:]:
            out = out + x
        return out

    def mul(self, *xs: Any) -> Any:
        if not xs:
            return z3.RealVal(1)
        out = xs[0]
        for x in xs[1:]:
            out = out * x
        return out

    def neg(self, x: Any) -> Any:
        return -x

    def div(self, a: Any, b: Any) -> Any:
        return a / b

    def pow_int(self, base: Any, n: int) -> Any:
        if n == 0:
            return z3.RealVal(1)
        if n < 0:
            return z3.RealVal(1) / self.pow_int(base, -n)
        out = base
        for _ in range(n - 1):
            out = out * base
        return out

    def eq(self, a: Any, b: Any) -> Any:
        return a == b

    def lt(self, a: Any, b: Any) -> Any:
        return a < b

    def le(self, a: Any, b: Any) -> Any:
        return a <= b

    def gt(self, a: Any, b: Any) -> Any:
        return a > b

    def ge(self, a: Any, b: Any) -> Any:
        return a >= b

    def and_(self, *xs: Any) -> Any:
        return z3.And(*xs) if xs else z3.BoolVal(True)

    def or_(self, *xs: Any) -> Any:
        return z3.Or(*xs) if xs else z3.BoolVal(False)

    def not_(self, x: Any) -> Any:
        return z3.Not(x)

    def implies(self, a: Any, b: Any) -> Any:
        return z3.Implies(a, b)

    def uninterpreted(self, name: str, args: list[Any]) -> Any:
        domain = [z3.RealSort()] * len(args)
        decl = z3.Function(name, *domain, z3.RealSort())
        return decl(*args)


# ----------------------------------------------------------------------
# Solver session
# ----------------------------------------------------------------------

class Z3Backend:
    def __init__(
        self,
        session: MonomixSession,
        *,
        default_timeout_ms: int = 5000,
    ) -> None:
        _require_z3()
        self._monomix_session = session
        self._solver = z3.Solver()
        self._solver.set("timeout", default_timeout_ms)
        self._builder = Z3TermBuilder()
        self._translator = Translator(self._builder, session)

    def assume(self, e: Expr) -> None:
        self._solver.add(self._translator.to_backend(e))

    def push(self) -> None:
        self._solver.push()

    def pop(self) -> None:
        self._solver.pop()

    def declared_symbols(self) -> list[str]:
        return [name for (name, _sort) in self._translator._symbols.keys()]

    def decide(self, formula: Expr) -> DecideResult:
        self._solver.push()
        try:
            self._solver.add(self._translator.to_backend(formula))
            r = self._solver.check()
            if r == z3.sat:
                return Sat(model=_extract_model(self._solver.model()))
            if r == z3.unsat:
                return Unsat()
            return Unknown()
        finally:
            self._solver.pop()

    def prove(
        self,
        claim: Expr,
        *,
        assumptions: list[Expr] | None = None,
    ) -> ProveResult:
        self._solver.push()
        try:
            for a in assumptions or []:
                self._solver.add(self._translator.to_backend(a))
            self._solver.add(z3.Not(self._translator.to_backend(claim)))
            r = self._solver.check()
            if r == z3.unsat:
                return Proved()
            if r == z3.sat:
                return Refuted(counterexample=_extract_model(self._solver.model()))
            return Unknown()
        finally:
            self._solver.pop()


def _extract_model(model: Any) -> dict[str, Any]:
    from fractions import Fraction

    out: dict[str, Any] = {}
    for d in model:
        v = model[d]
        if z3.is_int_value(v):
            out[str(d)] = v.as_long()
        elif z3.is_rational_value(v):
            out[str(d)] = Fraction(v.numerator_as_long(), v.denominator_as_long())
        elif z3.is_bool(v):
            out[str(d)] = bool(v)
        else:
            out[str(d)] = v
    return out
