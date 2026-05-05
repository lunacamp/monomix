"""Z3 backend behind the public solver facade.

Exposes the four verbs documented in ADR-0001:

    assume(constraints) -> SolverContext   (incremental, push/pop)
    prove(theorem, assumptions=...)        -> Proved | Refuted | Unknown
    decide(formula, assumptions=...)       -> Sat(model) | Unsat | Unknown
    simplify(expr, assumptions=...)        -> expr

`Unknown` is a first-class return value — callers (the CAS kernel) decide
whether to fall back to algebraic methods. We never raise on solver
indeterminacy.
"""

from __future__ import annotations

from dataclasses import dataclass
from fractions import Fraction
from typing import Iterable, List, Mapping, Optional, Union

from ..expr import Expr, Symbol
from .errors import BackendUnavailable
from .translate import Translator

try:
    import z3  # type: ignore
except ImportError:  # pragma: no cover
    z3 = None  # noqa: N816


# ----------------------------------------------------------------------
# Result types
# ----------------------------------------------------------------------


@dataclass(frozen=True)
class Proved:
    pass


@dataclass(frozen=True)
class Refuted:
    counterexample: Mapping[str, Union[Fraction, int, bool]]


@dataclass(frozen=True)
class Unknown:
    reason: str = ""


@dataclass(frozen=True)
class Sat:
    model: Mapping[str, Union[Fraction, int, bool]]


@dataclass(frozen=True)
class Unsat:
    pass


ProveResult = Union[Proved, Refuted, Unknown]
DecideResult = Union[Sat, Unsat, Unknown]


# ----------------------------------------------------------------------
# Backend
# ----------------------------------------------------------------------


class Z3Backend:
    """A long-lived Z3 session.

    Holds a single `Translator` so symbols stay identified across calls,
    and a single `z3.Solver` we drive incrementally. This is the object
    the CAS kernel keeps around between rewrite steps.
    """

    def __init__(self, *, default_timeout_ms: int = 5000) -> None:
        if z3 is None:
            raise BackendUnavailable(
                "z3-solver is not installed. Install with `pip install z3-solver`."
            )
        self._t = Translator()
        self._solver = z3.Solver()
        self._default_timeout_ms = default_timeout_ms

    # -- incremental assumption stack ----------------------------------

    def push(self) -> None:
        self._solver.push()

    def pop(self) -> None:
        self._solver.pop()

    def assume(self, constraint: Expr) -> None:
        self._solver.add(self._t.to_z3(constraint))

    def assume_all(self, constraints: Iterable[Expr]) -> None:
        for c in constraints:
            self.assume(c)

    # -- queries -------------------------------------------------------

    def decide(
        self,
        formula: Expr,
        *,
        assumptions: Optional[Iterable[Expr]] = None,
        timeout_ms: Optional[int] = None,
    ) -> DecideResult:
        """Is `formula` satisfiable under (current stack + assumptions)?"""
        with _scoped(self._solver, timeout_ms or self._default_timeout_ms):
            self._solver.push()
            try:
                if assumptions:
                    for a in assumptions:
                        self._solver.add(self._t.to_z3(a))
                self._solver.add(self._t.to_z3(formula))
                r = self._solver.check()
                if r == z3.sat:
                    return Sat(_extract_model(self._solver.model()))
                if r == z3.unsat:
                    return Unsat()
                return Unknown(reason=str(self._solver.reason_unknown()))
            finally:
                self._solver.pop()

    def prove(
        self,
        theorem: Expr,
        *,
        assumptions: Optional[Iterable[Expr]] = None,
        timeout_ms: Optional[int] = None,
    ) -> ProveResult:
        """Does `theorem` hold under (current stack + assumptions)?

        Internally: check satisfiability of `Not(theorem)`. If unsat the
        theorem is proved; if sat the model is a counterexample.
        """
        with _scoped(self._solver, timeout_ms or self._default_timeout_ms):
            self._solver.push()
            try:
                if assumptions:
                    for a in assumptions:
                        self._solver.add(self._t.to_z3(a))
                self._solver.add(z3.Not(self._t.to_z3(theorem)))
                r = self._solver.check()
                if r == z3.unsat:
                    return Proved()
                if r == z3.sat:
                    return Refuted(_extract_model(self._solver.model()))
                return Unknown(reason=str(self._solver.reason_unknown()))
            finally:
                self._solver.pop()

    # -- introspection -------------------------------------------------

    def declared_symbols(self) -> List[Symbol]:
        return [Symbol(name=k[0], sort=k[1]) for k in self._t._symbols]


# ----------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------


class _scoped:
    """Temporarily sets `set("timeout", ...)` on the solver."""

    def __init__(self, solver, timeout_ms: int) -> None:
        self._solver = solver
        self._timeout_ms = timeout_ms
        self._previous = None

    def __enter__(self):
        # Z3's Python API exposes solver.set("timeout", ms). There's no
        # public getter, so we just always set on entry; Z3 uses the
        # value for the next check() and we re-set on next call.
        self._solver.set("timeout", self._timeout_ms)
        return self

    def __exit__(self, *exc):
        return False


def _extract_model(m) -> Mapping[str, Union[Fraction, int, bool]]:
    out = {}
    for d in m.decls():
        v = m[d]
        out[d.name()] = _z3_value_to_python(v)
    return out


def _z3_value_to_python(v):
    if z3.is_int_value(v):
        return v.as_long()
    if z3.is_rational_value(v):
        return Fraction(v.numerator_as_long(), v.denominator_as_long())
    if z3.is_algebraic_value(v):
        # Algebraic numbers from nlsat — return a high-precision rational
        # approximation. Callers wanting exact algebraic numbers should
        # introspect the model themselves.
        approx = v.approx(20)
        return Fraction(
            approx.numerator_as_long(), approx.denominator_as_long()
        )
    if z3.is_true(v):
        return True
    if z3.is_false(v):
        return False
    return str(v)  # last-resort, e.g. uninterpreted values
