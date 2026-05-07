"""Minimal expression IR.

Placeholder for the eventual Rust kernel. The shape here is deliberately
conservative — just enough structure for the SMT translator to have
something to dispatch on. When the Rust kernel lands, this module is
replaced by a thin wrapper around the Rust term type (likely via PyO3)
and the translator keeps working unchanged because it only relies on
the visitor protocol below.

Design notes
------------
* Expressions are immutable, hashable, and structurally compared.
* Numbers are `Rational(num, den)` rather than Python floats — the CAS
  layer must never silently introduce floating-point error.
* Symbols carry an explicit *sort* (real / integer / boolean). This is
  what lets the SMT translator pick the right Z3 sort without guessing.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from fractions import Fraction
from typing import Literal, Tuple, Union

Sort = Literal["real", "int", "bool"]


@dataclass(frozen=True)
class Symbol:
    name: str
    sort: Sort = "real"


@dataclass(frozen=True)
class Rational:
    value: Fraction

    @staticmethod
    def of(num: int, den: int = 1) -> "Rational":
        return Rational(Fraction(num, den))


@dataclass(frozen=True)
class BoolConst:
    value: bool


@dataclass(frozen=True)
class App:
    """Function application. `head` is a string opcode."""

    head: str
    args: Tuple["Expr", ...] = field(default_factory=tuple)


Expr = Union[Symbol, Rational, BoolConst, App]


# -- Builders --------------------------------------------------------------
# Tiny ergonomic helpers so tests don't have to nest App(...) manually.


def add(*xs: Expr) -> Expr:
    return App("+", tuple(xs))


def mul(*xs: Expr) -> Expr:
    return App("*", tuple(xs))


def sub(a: Expr, b: Expr) -> Expr:
    return App("-", (a, b))


def neg(a: Expr) -> Expr:
    return App("neg", (a,))


def pow_(base: Expr, exp: Expr) -> Expr:
    return App("^", (base, exp))


def eq(a: Expr, b: Expr) -> Expr:
    return App("=", (a, b))


def lt(a: Expr, b: Expr) -> Expr:
    return App("<", (a, b))


def le(a: Expr, b: Expr) -> Expr:
    return App("<=", (a, b))


def gt(a: Expr, b: Expr) -> Expr:
    return App(">", (a, b))


def ge(a: Expr, b: Expr) -> Expr:
    return App(">=", (a, b))


def and_(*xs: Expr) -> Expr:
    return App("and", tuple(xs))


def or_(*xs: Expr) -> Expr:
    return App("or", tuple(xs))


def not_(x: Expr) -> Expr:
    return App("not", (x,))


def implies(a: Expr, b: Expr) -> Expr:
    return App("=>", (a, b))


def call(head: str, *xs: Expr) -> Expr:
    """Generic function application, e.g. `call("sin", x)`."""
    return App(head, tuple(xs))


# Constants
TRUE = BoolConst(True)
FALSE = BoolConst(False)
