from __future__ import annotations

from monomix import CrossSessionError, Session  # noqa: F401  (used in later phases)


def test_session_yields_expr():
    s = Session()
    x = s.symbol("x")
    assert x.kind == "Symbol"


def test_expr_outlives_session_drop():
    s = Session()
    x = s.symbol("x")
    del s   # Session goes away; Expr should still be valid
    assert x.kind == "Symbol"
    assert repr(x) == "Expr(x)"


def test_expr_is_same_within_session():
    s = Session()
    x1 = s.symbol("x")
    x2 = s.symbol("x")
    assert x1.is_same(x2)


def test_context_manager():
    with Session() as s:
        x = s.symbol("x")
    assert x.kind == "Symbol"


def test_integer_constructor():
    s = Session()
    n = s.integer(42)
    assert n.kind == "SmallInt"


def test_rational_constructor():
    s = Session()
    half = s.rational(1, 2)
    assert half.kind == "Rational"


def test_parse_basic():
    s = Session()
    e = s.parse("x + 1")
    assert e.kind == "Add"
