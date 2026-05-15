"""Public exception hierarchy for monomix.

The actual exception classes are defined by the Rust binding crate
(via PyO3's `create_exception!`). This module re-exports them under
a stable Python import path so user code can write
`from monomix.errors import MonomixError`.
"""

from __future__ import annotations

from monomix._kernel import (
    CrossSessionError,
    EvalError,
    MonomixError,
    ParseError,
    UnsupportedError,
)

__all__ = [
    "MonomixError",
    "ParseError",
    "EvalError",
    "UnsupportedError",
    "CrossSessionError",
]
