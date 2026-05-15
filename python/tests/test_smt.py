"""SMT bridge tests, rewritten against the Rust-backed Expr."""

from __future__ import annotations

from fractions import Fraction

import pytest

from monomix import Session
from monomix.smt import (
    BackendUnavailable,
    Proved,
    Refuted,
    Sat,
    Unknown,
    Unsat,
    Unsupported,
    open_session,
)

try:
    import z3  # noqa: F401
except ImportError:
    pytest.skip("z3-solver not installed", allow_module_level=True)


# -- Session lifecycle ------------------------------------------------------

def test_smt_session_opens_and_closes():
    s = Session()
    with open_session(s) as smt:
        assert smt.declared_symbols() == []


# -- Linear real arithmetic ------------------------------------------------

def test_prove_simple_linear_inequality():
    s = Session()
    x, y = s.symbol("x"), s.symbol("y")
    zero = s.integer(0)
    with open_session(s) as smt:
        result = smt.prove((x + y) > zero, assumptions=[x > zero, y > zero])
        assert isinstance(result, Proved)


def test_refute_with_counterexample():
    s = Session()
    x = s.symbol("x")
    with open_session(s) as smt:
        result = smt.prove(x > s.integer(1), assumptions=[x > s.integer(0)])
        assert isinstance(result, Refuted)
        cx = result.counterexample
        assert "x" in cx
        val = cx["x"]
        assert val > 0
        assert val <= 1


# -- Nonlinear real arithmetic ---------------------------------------------

def test_square_is_nonneg():
    s = Session()
    x = s.symbol("x")
    with open_session(s) as smt:
        result = smt.prove(x ** s.integer(2) >= s.integer(0))
        assert isinstance(result, Proved)


def test_unit_disk_intersect_halfplane_is_satisfiable():
    s = Session()
    x, y = s.symbol("x"), s.symbol("y")
    formula = ((x ** s.integer(2) + y ** s.integer(2)) < s.integer(1)) & \
              ((x + y) > s.rational(1, 2))
    with open_session(s) as smt:
        result = smt.decide(formula)
        assert isinstance(result, Sat)
        xv, yv = result.model["x"], result.model["y"]
        assert xv * xv + yv * yv < 1
        assert xv + yv > Fraction(1, 2)


def test_unit_disk_disjoint_from_far_halfplane():
    s = Session()
    x, y = s.symbol("x"), s.symbol("y")
    formula = ((x ** s.integer(2) + y ** s.integer(2)) < s.integer(1)) & \
              ((x + y) > s.integer(10))
    with open_session(s) as smt:
        result = smt.decide(formula)
        assert isinstance(result, Unsat)


# -- Push/pop --------------------------------------------------------------

def test_push_pop_isolates_assumptions():
    s = Session()
    x = s.symbol("x")
    with open_session(s) as smt:
        smt.assume(x > s.integer(0))
        smt.push()
        smt.assume(x < s.integer(0))
        assert isinstance(smt.decide(x == x), Unsat)
        smt.pop()
        assert isinstance(smt.prove(x > s.integer(0)), Proved)


# -- Integer sort ----------------------------------------------------------

def test_integer_division_property():
    s = Session()
    s.declare("n", "int")
    n = s.symbol("n")
    with open_session(s) as smt:
        result = smt.prove((n + n) == (s.integer(2) * n))
        assert isinstance(result, Proved)


# -- Unsupported / uninterpreted ------------------------------------------

def test_symbolic_exponent_is_unsupported():
    s = Session()
    x, y = s.symbol("x"), s.symbol("y")
    with open_session(s) as smt:
        with pytest.raises(Unsupported):
            smt.prove((x ** y) == (x ** y))


def test_unknown_function_becomes_uninterpreted():
    s = Session()
    s.symbol("x")  # ensure x exists
    sin_x = s.parse("sin(x)")
    with open_session(s) as smt:
        result = smt.prove(sin_x == sin_x)
        assert isinstance(result, Proved)


# -- Cross-session refusal -------------------------------------------------

def test_cross_session_expr_raises_in_smt():
    from monomix import CrossSessionError
    s1 = Session()
    s2 = Session()
    x = s1.symbol("x")
    y = s2.symbol("y")
    with pytest.raises(CrossSessionError):
        _ = x < y  # caught at operator level before reaching SMT


# -- Backend availability --------------------------------------------------

def test_backend_unavailable_is_an_exception_class():
    assert issubclass(BackendUnavailable, Exception)


# Sanity: Unknown is exposed even though we don't directly assert it.
def _unused() -> Unknown:
    return Unknown()
