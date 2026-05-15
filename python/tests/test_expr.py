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
