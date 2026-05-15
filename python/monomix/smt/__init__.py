"""SMT bridge — translate monomix Expr into a backend solver.

The package ships the translator and protocol only. No backend is
included in source; users supply a `Backend` implementation that
adapts whichever SMT solver they want to drive. See
[`designs/smt.md`](../../../designs/smt.md) for the protocol contract
and the feature parity requirements any backend must satisfy.

Typical wiring (illustrative — written in the caller's project):

    from monomix import Session
    from monomix.smt import Translator, Proved
    from my_pkg.my_backend import MyTermBuilder, MySolver

    s = Session()
    x = s.symbol("x")
    backend = MyTermBuilder()
    translator = Translator(backend, s)
    solver = MySolver()
    solver.add(backend.not_(translator.to_backend(x * x >= s.integer(0))))
    result = Proved() if solver.check_is_unsat() else ...
"""

from __future__ import annotations

from .errors import (
    BackendUnavailable,
    SolverError,
    TranslationError,
    Unsupported,
)
from .results import (
    DecideResult,
    ProveResult,
    Proved,
    Refuted,
    Sat,
    Unknown,
    Unsat,
)
from .translate import Backend, Translator

__all__ = [
    "Backend",
    "Translator",
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
