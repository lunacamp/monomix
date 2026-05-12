# Golden Corpus

Each `.toml` file contains a list of `[[entries]]` with:
- `input`: a REDUCE-syntax expression statement (including terminator `;` or `$`)
- `expected`: the expected string output matching REDUCE's `.rlg` file (or, for
  manifests where structural verification is used, a re-parseable form of the
  expected result — see `golden_tests.rs` for the per-manifest convention)
- `ignore = true` + `ignore_reason`: this entry is skipped by the runner

All entries must be parseable by the Phase 1 grammar (no `for`, `procedure`,
`array`, `on`/`off`, implicit multiplication like `3x^4`).

## Why entries get marked `ignore = true`

There are two distinct reasons an entry might be ignored. They should be
distinguishable from `ignore_reason` text and have different lifetimes.

1. **Unimplemented feature** (most current ignores). The corpus carries the
   spec ahead of the implementation — entries like
   `ignore_reason = "df() result display not yet implemented"` or
   `"like-terms collection not yet in M1"` are TODOs that will flip to
   `ignore = false` once the missing piece lands. These do **not** require
   an entry in `divergences.toml`. Use plain prose explaining what's missing.

2. **Intentional REDUCE divergence** (rare). Monomix is fully implemented
   for this case but its output deliberately differs from REDUCE (display
   formatting, term ordering, etc.). These **do** require an entry in
   `divergences.toml`, and the `ignore_reason` should reference the
   divergence's `id`. Example:
   `ignore_reason = "symbol-ordering: REDUCE sorts alphabetically"`.

See [divergences.toml](divergences.toml) for the registry of category (2).
The runner does not currently enforce the link between `ignore_reason` and
`divergences.toml`; the convention is documentary.
