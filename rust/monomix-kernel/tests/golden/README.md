# Golden Corpus

Each `.toml` file contains a list of `[[entries]]` with:
- `input`: a REDUCE-syntax expression statement (including terminator `;` or `$`)
- `expected`: the expected string output matching REDUCE's `.rlg` file
- `ignore = true` + `ignore_reason`: known intentional divergence

All entries must be parseable by the Phase 1 grammar (no `for`, `procedure`,
`array`, `on`/`off`, implicit multiplication like `3x^4`).

See `divergences.toml` for all known divergence annotations.
