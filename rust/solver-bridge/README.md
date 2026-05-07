# solver-bridge (Phase 2 sketch)

This crate is **not yet buildable**. It pins down the API shape so the
Phase 2 work — replacing the Python-side translator with a native one —
is mechanical when the Rust kernel is ready.

When activated:

1. The crate is already a member of the root Cargo workspace
   (see `../../Cargo.toml`); nothing to do here.
2. Uncomment the `z3` dependency in this crate's `Cargo.toml`. Building
   `z3-sys` requires the Z3 C library; on Windows the cleanest path is
   `vcpkg install z3` and `cargo build` with `Z3_SYS_Z3_HEADER` pointing
   at the installed header. On Linux/macOS the upstream `z3` Rust crate
   builds it from source as a Cargo build script.
3. Implement the `Z3Backend` struct outlined in `src/lib.rs`'s
   `z3_backend` module behind the `z3` feature flag.
4. Wire the kernel's term visitor through the `SmtBackend` trait.
