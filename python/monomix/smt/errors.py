"""Error types for the SMT bridge.

Kept in their own module so callers can `except Unsupported` without
pulling in a backend.
"""


class SolverError(Exception):
    """Base class for SMT bridge failures."""


class BackendUnavailable(SolverError):
    """The user-supplied backend cannot be initialised.

    Raised by backend adapters (not by the bridge itself) when the
    underlying solver isn't installed or refuses to start.
    """


class Unsupported(SolverError):
    """The expression cannot be lowered to the chosen SMT theory.

    Callers above the bridge should catch this and fall back to
    algebraic methods rather than failing the whole computation.
    """


class TranslationError(SolverError):
    """A monomix Expr kind couldn't be translated.

    Typically means a new ExprNode variant landed in the kernel and
    the Translator's dispatch table hasn't been extended yet.
    """
