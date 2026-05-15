from __future__ import annotations

import pytest

from monomix import Session, simplify


def test_simplify_constant_folds():
    s = Session()
    e = s.parse("0 + x")
    result = simplify(e)
    assert result.is_same(s.symbol("x"))
