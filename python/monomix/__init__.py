"""Monomix — modern CAS rewrite of REDUCE."""

from __future__ import annotations

from monomix._kernel import (
    Expr,
    df,
    evaluate_numeric,
    expand,
    simplify,
    solve,
    sub,
)
from monomix.errors import (
    CrossSessionError,
    EvalError,
    MonomixError,
    ParseError,
    UnsupportedError,
)
from monomix.session import Session

__version__ = "0.0.1"

__all__ = [
    "Expr",
    "Session",
    "df",
    "evaluate_numeric",
    "expand",
    "simplify",
    "solve",
    "sub",
    "MonomixError",
    "ParseError",
    "EvalError",
    "UnsupportedError",
    "CrossSessionError",
]
