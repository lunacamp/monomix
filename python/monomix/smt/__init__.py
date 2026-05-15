"""SMT bridge — translate monomix Expr into a backend solver.

Callers above the bridge (the rewrite system, the assumption store, the
piecewise simplifier) import only from this module. The choice of
backend is hidden; the only backend shipped today is Z3, used as the
parity reference for the abstract Backend protocol.
"""

from __future__ import annotations

from contextlib import contextmanager
from typing import Iterator

from .errors import BackendUnavailable, SolverError, TranslationError, Unsupported
from .z3_backend import (
    DecideResult,
    ProveResult,
    Proved,
    Refuted,
    Sat,
    Unknown,
    Unsat,
    Z3Backend,
)

__all__ = [
    "open_session",
    "Z3Backend",
    "Proved",
    "Refuted",
    "Unknown",
    "Sat",
    "Unsat",
    "ProveResult",
    "DecideResult",
    "SolverError",
    "BackendUnavailable",
    "TranslationError",
    "Unsupported",
]


@contextmanager
def open_session(*, default_timeout_ms: int = 5000) -> Iterator[Z3Backend]:
    """Open a solver session.

    The session owns its own Z3 solver instance and translator. Symbols
    declared inside the session keep their identity for the duration.
    Use `push() / pop()` for nested assumption scopes.
    """
    backend = Z3Backend(default_timeout_ms=default_timeout_ms)
    try:
        yield backend
    finally:
        # Z3 cleans up its own native resources when the Python object
        # is collected; nothing to do here today, but the contextmanager
        # gives us a place to add tracing / metrics later.
        pass
