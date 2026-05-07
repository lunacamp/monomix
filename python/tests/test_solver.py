"""End-to-end tests / runnable demos for the SMT bridge.

These also serve as the primary documentation of the API: each test is
a small worked example of a CAS subproblem we expect Z3 to handle.
"""

from __future__ import annotations

from fractions import Fraction

import pytest

from monomix.expr import (
    Rational,
    Symbol,
    add,
    and_,
    eq,
    ge,
    gt,
    le,
    lt,
    mul,
    not_,
    pow_,
    sub,
)
from monomix.solver import (
    BackendUnavailable,
    Proved,
    Refuted,
    Sat,
    Unsat,
    Unsupported,
    open_session,
)

# Skip the whole module if z3 isn't installed in this environment.
try:
    import z3  # noqa: F401
except ImportError:
    pytest.skip("z3-solver not installed", allow_module_level=True)


# ----------------------------------------------------------------------
# Trivial sanity
# ----------------------------------------------------------------------


def test_session_opens_and_closes():
    with open_session() as s:
        assert s.declared_symbols() == []


# ----------------------------------------------------------------------
# Linear real arithmetic
# ----------------------------------------------------------------------


def test_prove_simple_linear_inequality():
    """If x > 0 and y > 0, then x + y > 0."""
    x, y = Symbol("x"), Symbol("y")
    zero = Rational.of(0)
    with open_session() as s:
        result = s.prove(
            gt(add(x, y), zero),
            assumptions=[gt(x, zero), gt(y, zero)],
        )
        assert isinstance(result, Proved)


def test_refute_with_counterexample():
    """The claim 'x > 0 implies x > 1' is false; we expect a counterexample."""
    x = Symbol("x")
    result_obj = None
    with open_session() as s:
        result_obj = s.prove(
            gt(x, Rational.of(1)),
            assumptions=[gt(x, Rational.of(0))],
        )
    assert isinstance(result_obj, Refuted)
    cx = result_obj.counterexample
    assert "x" in cx
    # The counterexample must satisfy 0 < x <= 1
    val = cx["x"]
    assert isinstance(val, (int, Fraction))
    assert val > 0
    assert val <= 1


# ----------------------------------------------------------------------
# Nonlinear real arithmetic — Z3's nlsat fragment
# ----------------------------------------------------------------------


def test_square_is_nonneg():
    """For all real x: x^2 >= 0."""
    x = Symbol("x")
    with open_session() as s:
        result = s.prove(ge(pow_(x, Rational.of(2)), Rational.of(0)))
        assert isinstance(result, Proved)


def test_unit_disk_intersect_halfplane_is_satisfiable():
    """Find (x, y) with x^2 + y^2 < 1 and x + y > 1/2."""
    x, y = Symbol("x"), Symbol("y")
    formula = and_(
        lt(add(pow_(x, Rational.of(2)), pow_(y, Rational.of(2))), Rational.of(1)),
        gt(add(x, y), Rational.of(1, 2)),
    )
    with open_session() as s:
        result = s.decide(formula)
        assert isinstance(result, Sat)
        m = result.model
        xv, yv = m["x"], m["y"]
        assert xv * xv + yv * yv < 1
        assert xv + yv > Fraction(1, 2)


def test_unit_disk_disjoint_from_far_halfplane():
    """No (x, y) with x^2 + y^2 < 1 and x + y > 10."""
    x, y = Symbol("x"), Symbol("y")
    formula = and_(
        lt(add(pow_(x, Rational.of(2)), pow_(y, Rational.of(2))), Rational.of(1)),
        gt(add(x, y), Rational.of(10)),
    )
    with open_session() as s:
        result = s.decide(formula)
        assert isinstance(result, Unsat)


# ----------------------------------------------------------------------
# Incremental assumption stack
# ----------------------------------------------------------------------


def test_push_pop_isolates_assumptions():
    x = Symbol("x")
    with open_session() as s:
        s.assume(gt(x, Rational.of(0)))
        s.push()
        s.assume(lt(x, Rational.of(0)))  # contradicts the previous
        # Inner scope: x > 0 and x < 0 -> Unsat for any formula
        assert isinstance(s.decide(eq(x, x)), Unsat)
        s.pop()
        # Outer scope: only x > 0 holds
        assert isinstance(s.prove(gt(x, Rational.of(0))), Proved)


# ----------------------------------------------------------------------
# Mixed-sort: integers
# ----------------------------------------------------------------------


def test_integer_division_property():
    """For all integers n: n + n = 2n. Tests integer sort wiring."""
    n = Symbol("n", "int")
    with open_session() as s:
        result = s.prove(eq(add(n, n), mul(Rational.of(2), n)))
        assert isinstance(result, Proved)


# ----------------------------------------------------------------------
# Unsupported / fallback behaviour
# ----------------------------------------------------------------------


def test_symbolic_exponent_is_unsupported():
    """The translator must refuse symbolic exponents up front."""
    x, y = Symbol("x"), Symbol("y")
    with open_session() as s:
        with pytest.raises(Unsupported):
            s.prove(eq(pow_(x, y), pow_(x, y)))


def test_unknown_function_becomes_uninterpreted():
    """sin() isn't decidable; we declare it uninterpreted and accept Unknown.

    The point of the test is that translation succeeds (no exception)
    even though the proof attempt cannot conclude.
    """
    x = Symbol("x")
    from monomix.expr import call

    sin_x = call("sin", x)
    with open_session() as s:
        # f(x) = f(x) is trivially true even for uninterpreted f.
        result = s.prove(eq(sin_x, sin_x))
        assert isinstance(result, Proved)


# ----------------------------------------------------------------------
# Backend availability
# ----------------------------------------------------------------------


def test_backend_unavailable_is_an_exception_class():
    """Smoke test: BackendUnavailable is exposed and is an exception."""
    assert issubclass(BackendUnavailable, Exception)
