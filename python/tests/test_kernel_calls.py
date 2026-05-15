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
