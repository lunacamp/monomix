"""Solver-agnostic result vocabulary for the SMT bridge.

These dataclasses are what `prove` and `decide` operations return.
Backends construct them from their own native results; the bridge
treats them as opaque tags. See `designs/smt.md` §3.2.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass
class Proved:
    """`prove(claim)` succeeded — the claim is valid."""


@dataclass
class Refuted:
    """`prove(claim)` failed with a concrete counterexample.

    `counterexample` maps symbol names to the values that falsified
    the claim. Values are plain Python types (`int`, `Fraction`,
    `bool`, …) chosen by the backend.
    """

    counterexample: dict[str, Any]


@dataclass
class Sat:
    """`decide(formula)` found a satisfying assignment.

    `model` maps symbol names to their assigned values, with the
    same shape as `Refuted.counterexample`.
    """

    model: dict[str, Any]


@dataclass
class Unsat:
    """`decide(formula)` proved the formula has no model."""


@dataclass
class Unknown:
    """Backend gave up or timed out.

    A first-class result rather than an exception — symbolic engines
    need to distinguish "the backend says no" from "the backend gave
    up" so they know whether to fall back to algebraic methods.
    """


ProveResult = Proved | Refuted | Unknown
DecideResult = Sat | Unsat | Unknown
