"""Translate Monomix expression IR <-> Z3 AST.

The translator is intentionally small. It supports:

* Symbols with explicit sorts (real / int / bool)
* Rational constants (exact, never floats)
* Arithmetic: + - * / neg, integer-exponent power
* Comparisons: = < <= > >=
* Boolean: and or not =>
* Unknown function symbols (e.g. `sin`) are translated as *uninterpreted*
  functions on real -> real, which is sound but incomplete: Z3 cannot
  decide formulas involving them, but it also won't lie about them.

Anything else raises `Unsupported` so the caller can fall back to
algebraic methods. Adding new heads is a matter of editing the dispatch
table at the bottom of this file.
"""

from __future__ import annotations

from fractions import Fraction
from typing import Callable, Dict

from ..expr import App, BoolConst, Expr, Rational, Symbol
from .errors import TranslationError, Unsupported

try:
    import z3  # type: ignore
except ImportError as e:  # pragma: no cover - tested via BackendUnavailable
    z3 = None  # noqa: N816
    _IMPORT_ERROR = e
else:
    _IMPORT_ERROR = None


def _require_z3():
    if z3 is None:
        from .errors import BackendUnavailable

        raise BackendUnavailable(
            "z3-solver is not installed. Install with `pip install z3-solver`."
        ) from _IMPORT_ERROR


class Translator:
    """Stateful translator: caches Z3 declarations for symbols/functions.

    Reuse a single translator instance across a logical session so that
    `x` declared once stays the same Z3 constant across many formulas
    and a `push/pop` scope still refers to the same variables.
    """

    def __init__(self) -> None:
        _require_z3()
        self._symbols: Dict[tuple, "z3.ExprRef"] = {}
        self._uninterpreted_funcs: Dict[tuple, "z3.FuncDeclRef"] = {}

    # ------------------------------------------------------------------
    # Symbol / function declaration cache
    # ------------------------------------------------------------------

    def _declare_symbol(self, sym: Symbol) -> "z3.ExprRef":
        key = (sym.name, sym.sort)
        if key in self._symbols:
            return self._symbols[key]
        if sym.sort == "real":
            ref = z3.Real(sym.name)
        elif sym.sort == "int":
            ref = z3.Int(sym.name)
        elif sym.sort == "bool":
            ref = z3.Bool(sym.name)
        else:  # pragma: no cover - guarded by the Sort literal
            raise TranslationError(f"unknown sort {sym.sort!r}")
        self._symbols[key] = ref
        return ref

    def _declare_uninterpreted(self, head: str, arity: int) -> "z3.FuncDeclRef":
        key = (head, arity)
        if key in self._uninterpreted_funcs:
            return self._uninterpreted_funcs[key]
        domain = [z3.RealSort()] * arity
        decl = z3.Function(head, *domain, z3.RealSort())
        self._uninterpreted_funcs[key] = decl
        return decl

    # ------------------------------------------------------------------
    # IR -> Z3
    # ------------------------------------------------------------------

    def to_z3(self, e: Expr) -> "z3.ExprRef":
        if isinstance(e, Symbol):
            return self._declare_symbol(e)
        if isinstance(e, Rational):
            return _rational_to_z3(e.value)
        if isinstance(e, BoolConst):
            return z3.BoolVal(e.value)
        if isinstance(e, App):
            return self._app_to_z3(e)
        raise TranslationError(f"unhandled IR node: {type(e).__name__}")

    def _app_to_z3(self, e: App) -> "z3.ExprRef":
        handler = _DISPATCH.get(e.head)
        if handler is not None:
            return handler(self, e)
        # Fallback: treat unknown heads as uninterpreted real functions.
        # Sound but Z3 will report Unknown on most formulas using them.
        decl = self._declare_uninterpreted(e.head, len(e.args))
        return decl(*[self.to_z3(a) for a in e.args])


def _rational_to_z3(q: Fraction) -> "z3.ExprRef":
    if q.denominator == 1:
        return z3.RealVal(q.numerator)
    return z3.Q(q.numerator, q.denominator)


# ----------------------------------------------------------------------
# Dispatch table for known opcodes
# ----------------------------------------------------------------------


def _h_add(t: Translator, e: App) -> "z3.ExprRef":
    if not e.args:
        return z3.RealVal(0)
    args = [t.to_z3(a) for a in e.args]
    out = args[0]
    for a in args[1:]:
        out = out + a
    return out


def _h_mul(t: Translator, e: App) -> "z3.ExprRef":
    if not e.args:
        return z3.RealVal(1)
    args = [t.to_z3(a) for a in e.args]
    out = args[0]
    for a in args[1:]:
        out = out * a
    return out


def _h_sub(t: Translator, e: App) -> "z3.ExprRef":
    if len(e.args) != 2:
        raise TranslationError("'-' expects exactly 2 args (use 'neg' for unary)")
    return t.to_z3(e.args[0]) - t.to_z3(e.args[1])


def _h_neg(t: Translator, e: App) -> "z3.ExprRef":
    if len(e.args) != 1:
        raise TranslationError("'neg' expects exactly 1 arg")
    return -t.to_z3(e.args[0])


def _h_pow(t: Translator, e: App) -> "z3.ExprRef":
    if len(e.args) != 2:
        raise TranslationError("'^' expects exactly 2 args")
    base, exp = e.args
    z3_base = t.to_z3(base)
    # Z3 supports general real exponents but the nlsat decision procedure
    # only handles integer exponents. We allow integer-rational exponents
    # and reject arbitrary symbolic exponents up front.
    if isinstance(exp, Rational) and exp.value.denominator == 1:
        n = exp.value.numerator
        if n == 0:
            return z3.RealVal(1)
        if n > 0:
            out = z3_base
            for _ in range(n - 1):
                out = out * z3_base
            return out
        # negative integer exponent: 1 / x^|n|
        return z3.RealVal(1) / _h_pow(t, App("^", (base, Rational(Fraction(-n)))))
    raise Unsupported(
        "non-integer or symbolic exponent — outside Z3's nlsat fragment"
    )


def _h_div(t: Translator, e: App) -> "z3.ExprRef":
    if len(e.args) != 2:
        raise TranslationError("'/' expects exactly 2 args")
    return t.to_z3(e.args[0]) / t.to_z3(e.args[1])


def _binop(op: Callable):
    def handler(t: Translator, e: App) -> "z3.ExprRef":
        if len(e.args) != 2:
            raise TranslationError(f"binary op expects 2 args, got {len(e.args)}")
        return op(t.to_z3(e.args[0]), t.to_z3(e.args[1]))
    return handler


def _nary_bool(op):
    def handler(t: Translator, e: App) -> "z3.ExprRef":
        return op(*[t.to_z3(a) for a in e.args])
    return handler


def _h_not(t: Translator, e: App) -> "z3.ExprRef":
    if len(e.args) != 1:
        raise TranslationError("'not' expects exactly 1 arg")
    return z3.Not(t.to_z3(e.args[0]))


_DISPATCH: Dict[str, Callable[[Translator, App], "z3.ExprRef"]] = {
    "+": _h_add,
    "*": _h_mul,
    "-": _h_sub,
    "neg": _h_neg,
    "^": _h_pow,
    "/": _h_div,
    "=": _binop(lambda a, b: a == b),
    "<": _binop(lambda a, b: a < b),
    "<=": _binop(lambda a, b: a <= b),
    ">": _binop(lambda a, b: a > b),
    ">=": _binop(lambda a, b: a >= b),
    "and": _nary_bool(lambda *xs: z3.And(*xs) if xs else z3.BoolVal(True)),
    "or": _nary_bool(lambda *xs: z3.Or(*xs) if xs else z3.BoolVal(False)),
    "not": _h_not,
    "=>": _binop(lambda a, b: z3.Implies(a, b)),
}
