from __future__ import annotations

import pytest

from monomix import Session


@pytest.fixture
def s():
    return Session()


@pytest.fixture
def x(s):
    return s.symbol("x")


@pytest.fixture
def y(s):
    return s.symbol("y")


def test_add(x, y):
    assert (x + y).kind == "Add"


def test_sub(x, y):
    e = x - y
    assert e.kind == "Add"  # x + (-y), flattens


def test_mul(x, y):
    assert (x * y).kind == "Mul"


def test_div(x, y):
    assert (x / y).kind == "Div"


def test_pow(x, s):
    assert (x ** s.integer(2)).kind == "Pow"


def test_neg(x):
    assert (-x).kind == "Neg"


def test_literal_coercion_add(x):
    e = x + 1
    assert e.kind == "Add"


def test_literal_coercion_radd(x):
    e = 1 + x
    assert e.kind == "Add"


def test_literal_coercion_mul(x):
    assert (2 * x).kind == "Mul"


def test_eq_builds_eq_node(x, y):
    e = x == y
    assert e.kind == "Eq"


def test_eq_self_is_true(x):
    assert bool(x == x) is True


def test_eq_different_symbols_is_false(x, y):
    assert bool(x == y) is False


def test_ne_builds_not_eq(x, y):
    e = x != y
    assert e.kind == "Not"


def test_bool_of_non_eq_raises(x, y):
    e = x + y
    with pytest.raises(TypeError):
        bool(e)


def test_hash_consistency(x, s):
    x2 = s.symbol("x")
    assert hash(x) == hash(x2)
    assert bool(x == x2)


def test_hash_differs_for_distinct(x, y):
    assert hash(x) != hash(y)


def test_dict_key(x, s):
    x2 = s.symbol("x")
    d = {x: "value"}
    assert d[x2] == "value"


def test_eq_with_int_literal(x):
    e = x == 0
    assert e.kind == "Eq"


def test_lt(x, y):
    assert (x < y).kind == "Lt"


def test_le(x, y):
    assert (x <= y).kind == "Le"


def test_gt(x, y):
    assert (x > y).kind == "Gt"


def test_ge(x, y):
    assert (x >= y).kind == "Ge"


def test_lt_bool_raises(x, y):
    with pytest.raises(TypeError):
        bool(x < y)
