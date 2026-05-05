"""Error types for the solver bridge.

Kept in their own module so callers can `except Unsupported` without
pulling in z3 itself.
"""


class SolverError(Exception):
    """Base class for solver bridge failures."""


class BackendUnavailable(SolverError):
    """The chosen backend (e.g. z3) is not installed in this environment."""


class Unsupported(SolverError):
    """The expression cannot be lowered to the chosen SMT theory.

    Callers above the bridge should catch this and fall back to
    algebraic methods rather than failing the whole computation.
    """


class TranslationError(SolverError):
    """A Monomix IR node couldn't be translated to Z3."""
