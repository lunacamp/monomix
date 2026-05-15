"""Verify that simplify releases the GIL.

Two simplifies on two distinct Sessions, run concurrently from two
Python threads, should not take ~2x the wall time of a single one.

Soft-floor: the assertion uses a generous tolerance because CI runners
vary. If this false-fails consistently, mark it skip-on-single-cpu and
document.
"""

from __future__ import annotations

import threading
import time

from monomix import Session, simplify


REPS = 200


def _heavy_expr(s: Session):
    x = s.symbol("x")
    y = s.symbol("y")
    expr = x
    for i in range(1, 60):
        expr = expr + (x ** s.integer(i)) * (y ** s.integer(i))
    return expr


def _busy(e):
    for _ in range(REPS):
        _ = simplify(e)


def test_simplify_releases_gil():
    s1, s2 = Session(), Session()
    e1, e2 = _heavy_expr(s1), _heavy_expr(s2)

    # Warm caches so JIT-like effects don't skew the first run
    _busy(e1)

    t0 = time.perf_counter()
    _busy(e1)
    _busy(e2)
    serial = time.perf_counter() - t0

    t1 = time.perf_counter()
    threads = [
        threading.Thread(target=_busy, args=(e1,)),
        threading.Thread(target=_busy, args=(e2,)),
    ]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    parallel = time.perf_counter() - t1

    assert parallel < serial, (
        f"parallel ({parallel:.3f}s) not faster than serial ({serial:.3f}s)"
    )
