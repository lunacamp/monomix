from __future__ import annotations

import pytest

from monomix import Session, df, simplify


def test_simplify_constant_folds():
    s = Session()
    e = s.parse("0 + x")
    result = simplify(e)
    assert result.is_same(s.symbol("x"))


def test_df_polynomial():
    s = Session()
    x = s.symbol("x")
    expr = x ** s.integer(3)
    d = df(expr, x)
    result = simplify(d)
    expected = s.integer(3) * (x ** s.integer(2))
    assert result.is_same(simplify(expected))


def test_df_unsupported_on_comparison():
    from monomix import UnsupportedError
    s = Session()
    x = s.symbol("x")
    y = s.symbol("y")
    with pytest.raises(UnsupportedError):
        df(x < y, x)


def test_df_cross_session_raises():
    from monomix import CrossSessionError
    s1 = Session()
    s2 = Session()
    e = s1.symbol("x") ** s1.integer(2)
    var = s2.symbol("x")
    with pytest.raises(CrossSessionError):
        df(e, var)


def test_expand_product():
    from monomix import expand
    s = Session()
    x = s.symbol("x")
    expr = (x + s.integer(1)) * (x + s.integer(1))
    result = expand(expr)
    assert result.kind == "Add"


def test_solve_linear():
    from monomix import solve
    s = Session()
    x = s.symbol("x")
    eq = (x * s.integer(2) - s.integer(4)) == s.integer(0)
    solutions = solve(eq, x)
    assert len(solutions) >= 1


def test_sub_replaces_symbol():
    from monomix import sub
    s = Session()
    x = s.symbol("x")
    expr = x + s.integer(1)
    result = sub({x: s.integer(5)}, expr)
    assert simplify(result).is_same(s.integer(6))


def test_evaluate_numeric_constant():
    from monomix import evaluate_numeric
    s = Session()
    e = s.integer(3) + s.integer(4)
    assert evaluate_numeric(e) == pytest.approx(7.0)


def test_evaluate_numeric_unbound_symbol_raises():
    from monomix import EvalError, evaluate_numeric
    s = Session()
    x = s.symbol("x")
    with pytest.raises(EvalError):
        evaluate_numeric(x)
