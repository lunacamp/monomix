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


def test_and(x, y):
    a = (x == 0)
    b = (y == 0)
    e = a & b
    assert e.kind == "And"


def test_or(x, y):
    a = (x == 0)
    b = (y == 0)
    e = a | b
    assert e.kind == "Or"


def test_invert_eq(x):
    a = (x == 0)
    e = ~a
    assert e.kind == "Not"


def test_cross_session_add_raises():
    from monomix import CrossSessionError
    s1 = Session()
    s2 = Session()
    a = s1.symbol("x")
    b = s2.symbol("y")
    with pytest.raises(CrossSessionError):
        _ = a + b


def test_cross_session_eq_raises():
    from monomix import CrossSessionError
    s1 = Session()
    s2 = Session()
    a = s1.symbol("x")
    b = s2.symbol("y")
    with pytest.raises(CrossSessionError):
        _ = (a == b)


def test_str_infix_atoms(s, x):
    assert str(x) == "x"
    assert str(s.integer(3)) == "3"
    assert str(s.rational(3, 4)) == "3/4"


def test_str_infix_product_and_power(s, x):
    e = (x + s.integer(1)) * (x + s.integer(2))
    assert str(e) == "(1 + x)*(x + 2)"
    assert str(x ** s.integer(2)) == "x^2"
    assert str((x + s.integer(1)) ** s.integer(2)) == "(1 + x)^2"


def test_str_subtraction_not_plus_minus(s, x, y):
    # Negative terms render with " - ", never " + -".
    assert str(x - y) == "x - y"
    assert str(x - s.integer(3) * y) == "x - 3*y"
    assert str(-x) == "-x"
    assert " + -" not in str(s.integer(3) * x - s.integer(1))


def test_str_relational_and_boolean(s, x, y):
    assert str(x < y) == "x < y"
    assert str(x >= y) == "x >= y"
    assert str(x == y) == "x = y"
    assert str(~(x < y)) == "~(x < y)"


def test_repr_stays_structural(s, x):
    # repr is the debug form; str is the math form. Keep them distinct.
    e = (x + s.integer(1)) * (x + s.integer(2))
    assert repr(e).startswith("Expr(")
    assert repr(e) != str(e)
