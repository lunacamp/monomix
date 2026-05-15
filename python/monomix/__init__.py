"""Monomix — modern CAS rewrite of REDUCE."""

from __future__ import annotations

from monomix._kernel import Expr
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
    "MonomixError",
    "ParseError",
    "EvalError",
    "UnsupportedError",
    "CrossSessionError",
]
