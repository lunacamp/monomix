"""Monomix — modern CAS rewrite of REDUCE.

This package is the Python-side facade. The numerical/symbolic hot path
will eventually live in a Rust kernel; for now we host a minimal
expression IR here so the SMT bridge has something to translate from.
"""

__version__ = "0.0.1"
