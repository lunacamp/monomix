//! Native Rust bridge to Z3 — Phase 2 sketch.
//!
//! This crate is **not yet buildable**. The `z3` dependency is commented
//! out in `Cargo.toml`; the types here exist so the kernel-side API is
//! pinned down and reviewed before we link the C library.
//!
//! Design parallels `python/monomix/solver/`:
//!
//! - `Translator` caches Z3 declarations for kernel symbols.
//! - `Backend` owns a single `z3::Solver` and exposes `assume`, `prove`,
//!   `decide`. Same verbs, same return shapes as the Python facade.
//! - `Result` types are tagged enums mirroring `Proved | Refuted | Unknown`
//!   and `Sat | Unsat | Unknown` from the Python side.
//!
//! The Monomix Rust kernel calls into this crate directly when it has a
//! decidable subproblem on the hot path; the Python facade calls it via
//! PyO3 only when the kernel is invoked from Python. The two paths share
//! the same Z3 context behind the scenes.

#![allow(dead_code)]

use std::collections::HashMap;
use thiserror::Error;

/// Sorts the kernel cares about. Mirrors `monomix.expr.Sort`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sort {
    Real,
    Int,
    Bool,
}

/// A kernel-side reference to a term. The actual term graph lives in the
/// Monomix Rust kernel; this crate only sees an opaque ID + a small
/// enum of ops that mirrors the Python IR's `App` heads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TermId(pub u32);

/// Errors surfaced to the kernel. `Unknown` is *not* an error — it's a
/// successful decision result (see `DecideResult`).
#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("z3 backend not linked in this build")]
    BackendUnavailable,
    #[error("expression cannot be lowered to SMT: {0}")]
    Unsupported(String),
    #[error("translation failure: {0}")]
    Translation(String),
}

#[derive(Debug, Clone)]
pub enum DecideResult {
    Sat(Model),
    Unsat,
    Unknown(String),
}

#[derive(Debug, Clone)]
pub enum ProveResult {
    Proved,
    Refuted(Model),
    Unknown(String),
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub bindings: HashMap<String, ModelValue>,
}

#[derive(Debug, Clone)]
pub enum ModelValue {
    Int(i64),
    Rational { num: i128, den: i128 },
    Bool(bool),
    /// For algebraic numbers from nlsat we return a high-precision
    /// rational approximation; the kernel can re-ask Z3 for an exact
    /// algebraic representation if needed.
    Algebraic { approx_num: i128, approx_den: i128, precision_bits: u32 },
    /// Last-resort textual representation for sorts we don't model.
    Opaque(String),
}

// ----------------------------------------------------------------------
// Backend trait — lets us swap CVC5 / Yices in behind the same surface.
// ----------------------------------------------------------------------

pub trait SmtBackend {
    fn push(&mut self);
    fn pop(&mut self);
    fn assume(&mut self, term: TermId) -> Result<(), BridgeError>;
    fn decide(&mut self, formula: TermId, timeout_ms: u32) -> Result<DecideResult, BridgeError>;
    fn prove(&mut self, theorem: TermId, timeout_ms: u32) -> Result<ProveResult, BridgeError>;
}

// ----------------------------------------------------------------------
// Z3 backend — placeholder.
// ----------------------------------------------------------------------

#[cfg(feature = "z3")]
pub mod z3_backend {
    //! Real implementation lands here when we link `z3`/`z3-sys`.
    //! Sketch:
    //!
    //! ```ignore
    //! use z3::{Config, Context, Solver, ast::{Ast, Real, Int, Bool}};
    //!
    //! pub struct Z3Backend<'ctx> {
    //!     ctx: &'ctx Context,
    //!     solver: Solver<'ctx>,
    //!     symbols: HashMap<(String, Sort), z3::ast::Dynamic<'ctx>>,
    //!     uninterpreted: HashMap<(String, usize), z3::FuncDecl<'ctx>>,
    //! }
    //! ```
    //!
    //! The translator walks the kernel's term graph (driven by a visitor
    //! the kernel exposes), dispatches on the head opcode, and folds
    //! into Z3 ASTs the same way `translate.py` does. Push/pop and the
    //! timeout knob map 1:1 to the Z3 Rust API.
}

// ----------------------------------------------------------------------
// Stub backend so the crate compiles without the z3 feature flag.
// ----------------------------------------------------------------------

pub struct StubBackend;

impl SmtBackend for StubBackend {
    fn push(&mut self) {}
    fn pop(&mut self) {}
    fn assume(&mut self, _term: TermId) -> Result<(), BridgeError> {
        Err(BridgeError::BackendUnavailable)
    }
    fn decide(&mut self, _formula: TermId, _timeout_ms: u32) -> Result<DecideResult, BridgeError> {
        Err(BridgeError::BackendUnavailable)
    }
    fn prove(&mut self, _theorem: TermId, _timeout_ms: u32) -> Result<ProveResult, BridgeError> {
        Err(BridgeError::BackendUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_reports_unavailable() {
        let mut b = StubBackend;
        let r = b.decide(TermId(0), 1000);
        assert!(matches!(r, Err(BridgeError::BackendUnavailable)));
    }
}
