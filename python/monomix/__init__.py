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

# Single source of truth: the version baked into the compiled extension
# (monomix-py's CARGO_PKG_VERSION). Avoids drift with a hardcoded literal.
from monomix._kernel import __version__ as __version__

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
