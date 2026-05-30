from __future__ import annotations

import pytest

from monomix import CrossSessionError, ParseError, Session  # noqa: F401  (used in later phases)


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


def test_parse_rejects_multiple_statements():
    s = Session()
    with pytest.raises(ParseError, match="expects a single expression"):
        s.parse("x; y")


def test_declare_sort():
    s = Session()
    s.declare("n", "int")
    assert s.sort_of("n") == "int"


def test_declare_default_real():
    s = Session()
    assert s.sort_of("x") == "real"


def test_declare_invalid_sort_raises():
    s = Session()
    with pytest.raises(ValueError):
        s.declare("x", "complex")  # type: ignore[arg-type]  # deliberate bad sort


def test_declare_with_explicit_real():
    s = Session()
    s.declare("y", "real")
    assert s.sort_of("y") == "real"


def test_assign_and_clear():
    s = Session()
    x = s.symbol("x")
    s.assign("a", x)
    assert "a" in s.bindings()
    s.clear("a")
    assert "a" not in s.bindings()


def test_clear_missing_is_noop():
    s = Session()
    s.clear("nope")  # should not raise


def test_bindings_returns_copy():
    s = Session()
    x = s.symbol("x")
    s.assign("a", x)
    d = s.bindings()
    d["a"] = s.symbol("z")  # mutating the returned dict
    # must not affect the session
    assert s.bindings()["a"].is_same(x)


def test_parse_resolves_bindings():
    from monomix import simplify
    s = Session()
    x = s.symbol("x")
    s.assign("a", x + s.integer(1))
    result = s.parse("a + 1")
    expected = x + s.integer(2)
    assert simplify(result).is_same(simplify(expected))
