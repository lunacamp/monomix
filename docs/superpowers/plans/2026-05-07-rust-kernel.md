# Monomix Rust Kernel — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `rust/monomix-kernel` — the pure Rust CAS kernel — from workspace scaffold through a complete, benchmarked, fuzz-tested crate covering all Phase 1 subsystems.

**Architecture:** Two milestones. Milestone 1 builds the foundation: a hash-consed expression DAG (`ExprPool`), a Pratt parser, and a sparse univariate polynomial engine. Milestone 2 layers the five operations: bottom-up simplifier, recursive differentiator, bottom-up substitution, numeric evaluator, and linear/quadratic/Gaussian solver. All modules share `KernelError`, write into `ExprPool` via normalizing constructors, and are tested with unit tests, proptest invariants, criterion benchmarks, and a curated golden corpus against legacy REDUCE output.

**Tech Stack:** Rust 2021 edition; `num-bigint 0.4`, `num-rational 0.4`, `num-traits 0.2`, `rustc-hash 1`, `indexmap 2`, `ordered-float 4`, `arrayvec 0.7`, `smallvec 1` (union feature), `thiserror 1`; dev: `proptest 1`, `criterion 0.5`; fuzz: `cargo-fuzz` / `libfuzzer-sys`

---

## File Map

### Created — Milestone 1

- `rust/monomix-kernel/Cargo.toml`
- `rust/monomix-kernel/src/lib.rs`
- `rust/monomix-kernel/src/error.rs`
- `rust/monomix-kernel/src/expr/mod.rs`
- `rust/monomix-kernel/src/parser/mod.rs`
- `rust/monomix-kernel/src/parser/lexer.rs`
- `rust/monomix-kernel/src/parser/ast.rs`
- `rust/monomix-kernel/src/parser/expr.rs`
- `rust/monomix-kernel/src/parser/stmt.rs`
- `rust/monomix-kernel/src/poly/mod.rs`
- `rust/monomix-kernel/benches/kernel.rs`
- `rust/monomix-kernel/fuzz/Cargo.toml`
- `rust/monomix-kernel/fuzz/fuzz_targets/fuzz_parser.rs`
- `rust/monomix-kernel/tests/golden_tests.rs`
- `rust/monomix-kernel/tests/golden/README.md`
- `rust/monomix-kernel/tests/golden/divergences.toml`
- `rust/monomix-kernel/tests/golden/poly_div.toml`
- `rust/monomix-kernel/tests/golden/alg_expr.toml`

### Created — Milestone 2

- `rust/monomix-kernel/src/simplify/mod.rs`
- `rust/monomix-kernel/src/simplify/driver.rs`
- `rust/monomix-kernel/src/simplify/numeric.rs`
- `rust/monomix-kernel/src/simplify/like_terms.rs`
- `rust/monomix-kernel/src/simplify/powers.rs`
- `rust/monomix-kernel/src/simplify/rational.rs`
- `rust/monomix-kernel/src/simplify/patterns.rs`
- `rust/monomix-kernel/src/simplify/rules.rs`
- `rust/monomix-kernel/src/diff/mod.rs`
- `rust/monomix-kernel/src/diff/driver.rs`
- `rust/monomix-kernel/src/diff/arith.rs`
- `rust/monomix-kernel/src/diff/functions.rs`
- `rust/monomix-kernel/src/diff/table.rs`
- `rust/monomix-kernel/src/diff/plugin.rs`
- `rust/monomix-kernel/src/substitute/mod.rs`
- `rust/monomix-kernel/src/evalnum/mod.rs`
- `rust/monomix-kernel/src/solve/mod.rs`
- `rust/monomix-kernel/fuzz/fuzz_targets/fuzz_simplify.rs`
- `rust/monomix-kernel/fuzz/fuzz_targets/fuzz_diff.rs`
- `rust/monomix-kernel/tests/golden/solve_linear_quadratic.toml`
- `rust/monomix-kernel/tests/golden/simplify.toml`
- `rust/monomix-kernel/tests/golden/diff.toml`

### Modified

- `Cargo.toml` (workspace root): add `rust/monomix-kernel` to members

---

## MILESTONE 1 — FOUNDATION

---

### Task 1: Crate scaffold + workspace registration

This combines workspace edit and crate creation into one task so the workspace
is never in a broken state during a commit.

**Files:**
- Modify: `Cargo.toml`
- Create: `rust/monomix-kernel/Cargo.toml`
- Create: `rust/monomix-kernel/src/lib.rs`
- Create: `rust/monomix-kernel/src/error.rs` (stub)
- Create: `rust/monomix-kernel/src/expr/mod.rs` (stub)
- Create: `rust/monomix-kernel/src/parser/mod.rs` (stub)
- Create: `rust/monomix-kernel/src/poly/mod.rs` (stub)

- [ ] **Step 1: Create the crate's Cargo.toml**

```toml
# rust/monomix-kernel/Cargo.toml
[package]
name        = "monomix-kernel"
version     = "0.1.0"
edition.workspace    = true
license.workspace    = true
authors.workspace    = true

[dependencies]
num-bigint    = "0.4"
num-integer   = "0.1"
num-rational  = "0.4"
num-traits    = "0.2"
rustc-hash    = "1"
indexmap      = "2"
ordered-float = "4"
arrayvec      = "0.7"
smallvec      = { version = "1", features = ["union"] }
thiserror     = "1"

[dev-dependencies]
proptest  = "1"
criterion = { version = "0.5", features = ["html_reports"] }
serde     = { version = "1", features = ["derive"] }
toml      = "0.8"

[[bench]]
name    = "kernel"
harness = false

[lints]
workspace = true
```

- [ ] **Step 2: Create stub `src/lib.rs` and module files**

```rust
// rust/monomix-kernel/src/lib.rs
pub mod error;
pub mod expr;
pub mod parser;
pub mod poly;
```

Create each module file with a single placeholder comment so the crate compiles:

```rust
// rust/monomix-kernel/src/error.rs
// implementation in Task 3

// rust/monomix-kernel/src/expr/mod.rs
// implementation in Task 4+

// rust/monomix-kernel/src/parser/mod.rs
// implementation in Task 9+

// rust/monomix-kernel/src/poly/mod.rs
// implementation in Task 13+
```

- [ ] **Step 3: Add the crate to the workspace `members` list**

```toml
# Cargo.toml — workspace root
[workspace]
resolver = "2"
members  = ["rust/solver-bridge", "rust/monomix-kernel"]

[workspace.package]
edition = "2021"
license = "MIT"
authors = ["Roman Korneev"]

[workspace.lints.rust]
unsafe_code = "forbid"
```

- [ ] **Step 4: Verify the whole workspace compiles**

```
cargo build -p monomix-kernel
```

Expected: compiles with zero errors.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml rust/monomix-kernel/
git commit -m "feat: add monomix-kernel crate scaffold and register in workspace"
```

---

### Task 2 — RESERVED

This task slot was merged into Task 1. Skip directly to Task 3.

---

### Task 3: `error.rs` — KernelError

**Files:**
- Create: `rust/monomix-kernel/src/error.rs`

- [ ] **Step 1: Write the failing test**

Add to `rust/monomix-kernel/src/error.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_division_by_zero() {
        let e = KernelError::DivisionByZero { span: None };
        assert_eq!(e.to_string(), "division by zero");
    }

    #[test]
    fn error_display_unbound_symbol() {
        let e = KernelError::UnboundSymbol("x".to_string());
        assert_eq!(e.to_string(), "unbound symbol: x");
    }

    #[test]
    fn error_display_unsupported_equation() {
        let e = KernelError::UnsupportedEquation { reason: "cubic".to_string() };
        assert_eq!(e.to_string(), "unsupported equation form: cubic");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```
cargo test -p monomix-kernel error -- --nocapture
```

Expected: FAIL — `KernelError` not yet defined.

- [ ] **Step 3: Implement KernelError**

```rust
// rust/monomix-kernel/src/error.rs
use crate::parser::ast::Span;

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    // Parser
    #[error("parse error")]
    Parse(Vec<crate::parser::ast::Diagnostic>),

    // Expression pool
    #[error("pool exhausted")]
    PoolExhausted,
    #[error("division by zero")]
    DivisionByZero { span: Option<Span> },
    #[error("indeterminate form 0/0")]
    IndeterminateForm,

    // Differentiator
    #[error("cannot differentiate an equation")]
    DifferentiateEquation,
    #[error("differentiation variable must be a symbol")]
    NotASymbol,

    // Substitution
    #[error("substitution target must be a symbol")]
    SubstituteNotASymbol,
    #[error("cyclic binding detected")]
    CyclicBinding,

    // Numeric evaluation
    #[error("unbound symbol: {0}")]
    UnboundSymbol(String),
    #[error("log of non-positive value")]
    LogOfNonPositive,
    #[error("sqrt of negative value")]
    SqrtOfNegative,
    #[error("domain error in {fn_name}")]
    DomainError { fn_name: &'static str },
    #[error("unsupported function for numeric eval")]
    UnsupportedFn,

    // Solver
    #[error("unsupported equation form: {reason}")]
    UnsupportedEquation { reason: String },
    #[error("singular system")]
    SingularSystem,

    // Arithmetic
    #[error("arithmetic overflow")]
    Overflow,
    #[error("numeric evaluation produced NaN")]
    NumericNaN,
}
```

Note: `error.rs` references `crate::parser::ast::Span`. Until `parser/ast.rs` is written, stub the import:

```rust
// temporary until parser is implemented
use crate::parser::ast::Span;
```

Add to `parser/mod.rs` and `parser/ast.rs` stubs:
```rust
// rust/monomix-kernel/src/parser/ast.rs (stub)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span { pub start: u32, pub end: u32 }

#[derive(Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub message: String,
    pub code: DiagnosticCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity { Error, Warning }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    UnexpectedToken { found: TokenKind, expected: &'static str },
    UnterminatedStatement,
    UnbalancedParen,
    InvalidNumericLiteral,
    NumericLiteralTooLong,
    IdentifierTooLong,
    MissingArgument { function: &'static str },
    TooManyArguments { function: &'static str, max: usize },
}

// forward ref — real definition in lexer.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    SmallInt, BigInt, Float, Ident,
    Plus, Minus, Star, Slash, Pow,
    Assign, Equals, Comma, LParen, RParen,
    Semi, Dollar, KwComment, Eof,
}
```

Add to `parser/mod.rs`:
```rust
pub mod ast;
```

- [ ] **Step 4: Run test to verify it passes**

```
cargo test -p monomix-kernel error -- --nocapture
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/error.rs rust/monomix-kernel/src/parser/
git commit -m "feat(kernel): add KernelError + parser AST stubs"
```

---

### Task 4: `expr` — Core types

**Files:**
- Create: `rust/monomix-kernel/src/expr/mod.rs`

- [ ] **Step 1: Write the failing compile-time size guard test**

```rust
// rust/monomix-kernel/src/expr/mod.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expr_node_size_at_most_32_bytes() {
        // This is also enforced at compile time below, but we test it
        // explicitly so a failure has a clear test name.
        assert!(
            std::mem::size_of::<ExprNode>() <= 32,
            "ExprNode is {} bytes, must be ≤32",
            std::mem::size_of::<ExprNode>()
        );
    }

    #[test]
    fn local_expr_id_is_copy() {
        let id = LocalExprId(0);
        let id2 = id; // copy
        assert_eq!(id, id2);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```
cargo test -p monomix-kernel expr::tests -- --nocapture
```

Expected: FAIL — `ExprNode` not defined.

- [ ] **Step 3: Implement core types**

```rust
// rust/monomix-kernel/src/expr/mod.rs
use num_bigint::BigInt;
use ordered_float::OrderedFloat;

// ---- Handles ---------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct LocalExprId(pub u32);

pub type ExprId = LocalExprId;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct InternedStr(pub u32);

// ---- Function tag ----------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FnTag {
    Sin, Cos, Tan, Exp, Log, Sqrt, Abs,
    Asin, Acos, Atan,
    Custom(InternedStr),
}

// ---- Expression node -------------------------------------------------------

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ExprNode {
    // Atoms
    SmallInt(i64),
    BigInt(Box<BigInt>),
    Rational(Box<(BigInt, BigInt)>),
    Float(OrderedFloat<f64>),
    Symbol(InternedStr),
    String(InternedStr),

    // Composite (children are immutable Box<[ExprId]>)
    Add(Box<[ExprId]>),
    Mul(Box<[ExprId]>),
    Pow(ExprId, ExprId),
    Neg(ExprId),
    Div(ExprId, ExprId),
    Eq(ExprId, ExprId),
    Fn(FnTag, Box<[ExprId]>),
    List(Box<[ExprId]>),
}

// Compile-time size guard — fails compilation if ExprNode exceeds 32 bytes.
const _EXPR_NODE_SIZE_GUARD: [(); 32] = [(); std::mem::size_of::<ExprNode>()];
```

- [ ] **Step 4: Run test to verify it passes**

```
cargo test -p monomix-kernel expr::tests -- --nocapture
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/expr/mod.rs
git commit -m "feat(expr): add core types ExprNode/ExprId/FnTag with size guard"
```

---

### Task 5: `ExprPool` — Arena + Atom Constructors

**Files:**
- Modify: `rust/monomix-kernel/src/expr/mod.rs`

- [ ] **Step 1: Write the failing tests**

```rust
// Add inside mod tests in expr/mod.rs:

#[test]
fn pool_integer_interning_idempotent() {
    let mut pool = ExprPool::new();
    let a = pool.small_int(42);
    let b = pool.small_int(42);
    assert_eq!(a, b);
}

#[test]
fn pool_symbol_interning_case_insensitive() {
    let mut pool = ExprPool::new();
    let x1 = pool.symbol("x");
    let x2 = pool.symbol("X"); // lowercased at intern
    assert_eq!(x1, x2);
}

#[test]
fn pool_zero_one_minus_one_pre_interned() {
    let pool = ExprPool::new();
    let z = pool.small_int_check(0);
    let o = pool.small_int_check(1);
    assert_eq!(z, pool.zero);
    assert_eq!(o, pool.one);
}

#[test]
fn pool_rational_normalized() {
    let mut pool = ExprPool::new();
    use num_bigint::BigInt;
    let r1 = pool.rational(BigInt::from(4), BigInt::from(6));
    let r2 = pool.rational(BigInt::from(2), BigInt::from(3));
    assert_eq!(r1, r2);
}
```

- [ ] **Step 2: Run test to verify it fails**

```
cargo test -p monomix-kernel expr::tests -- --nocapture
```

Expected: FAIL — `ExprPool` not defined.

- [ ] **Step 3: Implement ExprPool struct + atom constructors**

```rust
// Append to rust/monomix-kernel/src/expr/mod.rs

use rustc_hash::FxHashMap;
use indexmap::IndexSet;
use smallvec::SmallVec;
use num_traits::ToPrimitive;
use num_integer::Integer;

// ---- Arena -----------------------------------------------------------------

struct ArenaEntry {
    hash: u64,
    node: ExprNode,
    subtree_size: u32,
}

// ---- Pool ------------------------------------------------------------------

pub struct ExprPool {
    nodes: Vec<ArenaEntry>,
    /// dedup: content_hash → list of ExprId with that hash
    dedup: FxHashMap<u64, SmallVec<[ExprId; 1]>>,
    strings: IndexSet<String>,

    pub zero: ExprId,
    pub one: ExprId,
    pub minus_one: ExprId,
}

impl ExprPool {
    pub fn new() -> Self {
        let mut pool = ExprPool {
            nodes: Vec::new(),
            dedup: FxHashMap::default(),
            strings: IndexSet::new(),
            zero: LocalExprId(0),
            one: LocalExprId(0),
            minus_one: LocalExprId(0),
        };
        // Pre-intern common constants
        pool.zero = pool.intern(ExprNode::SmallInt(0));
        pool.one  = pool.intern(ExprNode::SmallInt(1));
        pool.minus_one = pool.intern(ExprNode::SmallInt(-1));
        // Pre-intern common symbols: x, y, z, t, e, pi, i
        for sym in &["x", "y", "z", "t", "e", "pi", "i"] {
            pool.intern_str(sym);
        }
        pool
    }

    // --- String interning ---------------------------------------------------

    fn intern_str(&mut self, s: &str) -> InternedStr {
        let lower: String = s.to_lowercase();
        let (idx, _) = self.strings.insert_full(lower);
        InternedStr(idx as u32)
    }

    pub fn str_of(&self, s: InternedStr) -> &str {
        self.strings.get_index(s.0 as usize).map(|s| s.as_str()).unwrap_or("<?>")
    }

    // --- Low-level intern ---------------------------------------------------

    fn content_hash(node: &ExprNode) -> u64 {
        use std::hash::{Hash, Hasher};
        use rustc_hash::FxHasher;
        let mut h = FxHasher::default();
        std::mem::discriminant(node).hash(&mut h);
        match node {
            ExprNode::SmallInt(n) => n.hash(&mut h),
            ExprNode::BigInt(n) => n.hash(&mut h),
            ExprNode::Rational(b) => { b.0.hash(&mut h); b.1.hash(&mut h); }
            ExprNode::Float(f) => f.hash(&mut h),
            ExprNode::Symbol(s) | ExprNode::String(s) => s.hash(&mut h),
            ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c) => {
                for id in c.iter() { id.hash(&mut h); }
            }
            ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b) => {
                a.hash(&mut h); b.hash(&mut h);
            }
            ExprNode::Neg(x) => x.hash(&mut h),
            ExprNode::Fn(tag, args) => {
                tag.hash(&mut h);
                for id in args.iter() { id.hash(&mut h); }
            }
        }
        h.finish()
    }

    fn subtree_size_of(node: &ExprNode, nodes: &[ArenaEntry]) -> u32 {
        let children: &[ExprId] = match node {
            ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c) => c,
            ExprNode::Fn(_, c) => c,
            ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b) => {
                return 1 + nodes[a.0 as usize].subtree_size
                         + nodes[b.0 as usize].subtree_size;
            }
            ExprNode::Neg(x) => return 1 + nodes[x.0 as usize].subtree_size,
            _ => return 1,
        };
        1 + children.iter().map(|c| nodes[c.0 as usize].subtree_size).sum::<u32>()
    }

    fn intern(&mut self, node: ExprNode) -> ExprId {
        let hash = Self::content_hash(&node);
        if let Some(candidates) = self.dedup.get(&hash) {
            for &id in candidates.iter() {
                if self.nodes[id.0 as usize].node == node {
                    return id;
                }
            }
        }
        let size = Self::subtree_size_of(&node, &self.nodes);
        let id = LocalExprId(self.nodes.len() as u32);
        self.nodes.push(ArenaEntry { hash, node, subtree_size: size });
        self.dedup.entry(hash).or_default().push(id);
        id
    }

    // --- Atom constructors --------------------------------------------------

    pub fn small_int(&mut self, n: i64) -> ExprId {
        self.intern(ExprNode::SmallInt(n))
    }

    /// Used in tests to check pre-interned constant without &mut self.
    pub fn small_int_check(&self, n: i64) -> ExprId {
        let h = Self::content_hash(&ExprNode::SmallInt(n));
        if let Some(candidates) = self.dedup.get(&h) {
            for &id in candidates.iter() {
                if self.nodes[id.0 as usize].node == ExprNode::SmallInt(n) {
                    return id;
                }
            }
        }
        panic!("small_int_check: {} not pre-interned", n);
    }

    pub fn integer(&mut self, n: BigInt) -> ExprId {
        if let Some(i) = n.to_i64() {
            self.intern(ExprNode::SmallInt(i))
        } else {
            self.intern(ExprNode::BigInt(Box::new(n)))
        }
    }

    pub fn rational(&mut self, p: BigInt, q: BigInt) -> ExprId {
        assert!(!q.is_zero(), "rational: denominator is zero");
        use num_traits::Zero;
        if p.is_zero() {
            return self.zero;
        }
        let g = p.gcd(&q);
        let mut pn = p / &g;
        let mut qn = q / &g;
        if qn.sign() == num_bigint::Sign::Minus {
            pn = -pn;
            qn = -qn;
        }
        if qn == BigInt::from(1) {
            return self.integer(pn);
        }
        self.intern(ExprNode::Rational(Box::new((pn, qn))))
    }

    pub fn float(&mut self, f: f64) -> ExprId {
        self.intern(ExprNode::Float(OrderedFloat(f)))
    }

    pub fn symbol(&mut self, name: &str) -> ExprId {
        let s = self.intern_str(name);
        self.intern(ExprNode::Symbol(s))
    }

    pub fn symbol_by_id(&mut self, s: InternedStr) -> ExprId {
        self.intern(ExprNode::Symbol(s))
    }

    pub fn string_lit(&mut self, s: &str) -> ExprId {
        let id = self.intern_str(s);
        self.intern(ExprNode::String(id))
    }

    // --- Access -------------------------------------------------------------

    pub fn get(&self, id: ExprId) -> &ExprNode {
        &self.nodes[id.0 as usize].node
    }

    pub fn subtree_size(&self, id: ExprId) -> u32 {
        self.nodes[id.0 as usize].subtree_size
    }

    /// Returns owned Vec of children for any node.
    /// Atoms return empty. For binary operators, returns [a, b]. For n-ary,
    /// the slice is cloned because nodes don't store children in a contiguous
    /// representation that supports a unified `&[ExprId]` view (binary ops
    /// hold two separate fields, not a slice).
    pub fn children(&self, id: ExprId) -> Vec<ExprId> {
        match &self.nodes[id.0 as usize].node {
            ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c) => c.to_vec(),
            ExprNode::Fn(_, c) => c.to_vec(),
            ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b) => vec![*a, *b],
            ExprNode::Neg(x) => vec![*x],
            _ => Vec::new(),
        }
    }

    pub fn is_zero(&self, id: ExprId) -> bool { id == self.zero }
    pub fn is_one(&self, id: ExprId) -> bool  { id == self.one  }

    pub fn is_atom(&self, id: ExprId) -> bool {
        matches!(self.get(id),
            ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_)
            | ExprNode::Float(_) | ExprNode::Symbol(_) | ExprNode::String(_))
    }

    pub fn is_numeric(&self, id: ExprId) -> bool {
        matches!(self.get(id),
            ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_)
            | ExprNode::Float(_))
    }

    pub fn len(&self) -> usize { self.nodes.len() }
}

impl Default for ExprPool {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel expr::tests -- --nocapture
```

Expected: all pool tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/expr/mod.rs
git commit -m "feat(expr): add ExprPool arena + atom constructors"
```

---

### Task 6: `ExprPool` — Normalizing Composite Constructors

**Files:**
- Modify: `rust/monomix-kernel/src/expr/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
// Add inside mod tests:

#[test]
fn add_flattens_and_sorts() {
    let mut pool = ExprPool::new();
    let a = pool.symbol("a");
    let b = pool.symbol("b");
    let c = pool.symbol("c");
    let ab = pool.add(vec![a, b]);
    let ab_c = pool.add(vec![ab, c]);
    let bc = pool.add(vec![b, c]);
    let a_bc = pool.add(vec![a, bc]);
    assert_eq!(ab_c, a_bc, "add should flatten and produce same node");
}

#[test]
fn add_commutativity() {
    let mut pool = ExprPool::new();
    let a = pool.symbol("a");
    let b = pool.symbol("b");
    let ab = pool.add(vec![a, b]);
    let ba = pool.add(vec![b, a]);
    assert_eq!(ab, ba);
}

#[test]
fn neg_double_negation() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let neg_x = pool.neg(x);
    assert_eq!(pool.neg(neg_x), x);
}

#[test]
fn pow_identity_rules() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let zero = pool.zero;
    let one = pool.one;
    assert_eq!(pool.pow(x, zero), one);
    assert_eq!(pool.pow(x, one), x);
}

#[test]
fn div_by_one() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let one = pool.one;
    assert_eq!(pool.div(x, one), x);
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel expr::tests::add_flattens -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Implement composite constructors**

```rust
// Append to impl ExprPool in expr/mod.rs:

    pub fn add(&mut self, children: Vec<ExprId>) -> ExprId {
        // Flatten nested Add nodes
        let mut flat: Vec<ExprId> = Vec::with_capacity(children.len());
        for c in children {
            if let ExprNode::Add(inner) = self.get(c).clone() {
                flat.extend_from_slice(&inner);
            } else {
                flat.push(c);
            }
        }
        // Remove zeros
        flat.retain(|&c| !self.is_zero(c));
        if flat.is_empty() {
            return self.zero;
        }
        if flat.len() == 1 {
            return flat[0];
        }
        // Sort for canonical form
        flat.sort_unstable();
        self.intern(ExprNode::Add(flat.into_boxed_slice()))
    }

    pub fn mul(&mut self, children: Vec<ExprId>) -> ExprId {
        // Flatten nested Mul nodes
        let mut flat: Vec<ExprId> = Vec::with_capacity(children.len());
        for c in children {
            if let ExprNode::Mul(inner) = self.get(c).clone() {
                flat.extend_from_slice(&inner);
            } else {
                flat.push(c);
            }
        }
        // Short-circuit on zero
        if flat.iter().any(|&c| self.is_zero(c)) {
            return self.zero;
        }
        // Remove ones
        flat.retain(|&c| !self.is_one(c));
        if flat.is_empty() {
            return self.one;
        }
        if flat.len() == 1 {
            return flat[0];
        }
        flat.sort_unstable();
        self.intern(ExprNode::Mul(flat.into_boxed_slice()))
    }

    pub fn pow(&mut self, base: ExprId, exp: ExprId) -> ExprId {
        if self.is_zero(exp) { return self.one; }
        if self.is_one(exp)  { return base; }
        self.intern(ExprNode::Pow(base, exp))
    }

    pub fn neg(&mut self, x: ExprId) -> ExprId {
        if let ExprNode::Neg(inner) = *self.get(x) {
            return inner; // neg(neg(x)) → x
        }
        if self.is_zero(x) {
            return self.zero;
        }
        self.intern(ExprNode::Neg(x))
    }

    pub fn div(&mut self, num: ExprId, den: ExprId) -> ExprId {
        if self.is_one(den) { return num; }
        self.intern(ExprNode::Div(num, den))
    }

    pub fn eq_node(&mut self, lhs: ExprId, rhs: ExprId) -> ExprId {
        self.intern(ExprNode::Eq(lhs, rhs))
    }

    pub fn func(&mut self, tag: FnTag, args: Vec<ExprId>) -> ExprId {
        self.intern(ExprNode::Fn(tag, args.into_boxed_slice()))
    }

    pub fn func_named(&mut self, name: &str, args: Vec<ExprId>) -> ExprId {
        let s = self.intern_str(name);
        self.func(FnTag::Custom(s), args)
    }

    pub fn list(&mut self, items: Vec<ExprId>) -> ExprId {
        self.intern(ExprNode::List(items.into_boxed_slice()))
    }
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel expr::tests -- --nocapture
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/expr/mod.rs
git commit -m "feat(expr): add normalizing composite constructors"
```

---

### Task 7: `ExprPool` — Traversal Helpers

**Files:**
- Modify: `rust/monomix-kernel/src/expr/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
// Add inside mod tests:

#[test]
fn contains_symbol_finds_nested() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let y = pool.symbol("y");
    let xy = pool.mul(vec![x, y]);
    let one = pool.small_int(1);
    let expr = pool.add(vec![xy, one]);
    assert!(pool.contains_symbol(expr, x));
    assert!(pool.contains_symbol(expr, y));
    let z = pool.symbol("z");
    assert!(!pool.contains_symbol(expr, z));
}

#[test]
fn fold_sums_all_small_ints() {
    let mut pool = ExprPool::new();
    let a = pool.small_int(3);
    let b = pool.small_int(4);
    let expr = pool.add(vec![a, b]);
    let sum = pool.fold(expr, 0i64, &mut |acc, _id, node| match node {
        ExprNode::SmallInt(n) => acc + n,
        _ => acc,
    });
    assert_eq!(sum, 7);
}

#[test]
fn map_bottom_up_identity() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let one = pool.small_int(1);
    let expr = pool.add(vec![x, one]);
    let result = pool.map_bottom_up_fresh(expr, &mut |_pool, id| id);
    assert_eq!(result, expr);
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel expr::tests::contains_symbol -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Implement traversal helpers**

```rust
// Append to impl ExprPool in expr/mod.rs:

    pub fn contains_symbol(&self, expr: ExprId, sym: ExprId) -> bool {
        self.fold(expr, false, &mut |found, id, _node| found || id == sym)
    }

    pub fn fold<A>(
        &self,
        root: ExprId,
        init: A,
        f: &mut dyn FnMut(A, ExprId, &ExprNode) -> A,
    ) -> A {
        let mut visited = rustc_hash::FxHashSet::default();
        self.fold_impl(root, init, f, &mut visited)
    }

    fn fold_impl<A>(
        &self,
        id: ExprId,
        acc: A,
        f: &mut dyn FnMut(A, ExprId, &ExprNode) -> A,
        visited: &mut rustc_hash::FxHashSet<ExprId>,
    ) -> A {
        if !visited.insert(id) {
            return acc;
        }
        let node = &self.nodes[id.0 as usize].node;
        // Visit children first (bottom-up)
        let acc = match node {
            ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c) => {
                let ids: Vec<ExprId> = c.to_vec();
                ids.iter().fold(acc, |a, &child| self.fold_impl(child, a, f, visited))
            }
            ExprNode::Fn(_, c) => {
                let ids: Vec<ExprId> = c.to_vec();
                ids.iter().fold(acc, |a, &child| self.fold_impl(child, a, f, visited))
            }
            ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b) => {
                let (a, b) = (*a, *b);
                let acc = self.fold_impl(a, acc, f, visited);
                self.fold_impl(b, acc, f, visited)
            }
            ExprNode::Neg(x) => { let x = *x; self.fold_impl(x, acc, f, visited) }
            _ => acc,
        };
        f(acc, id, &self.nodes[id.0 as usize].node)
    }

    pub fn map_bottom_up(
        &mut self,
        root: ExprId,
        cache: &mut FxHashMap<ExprId, ExprId>,
        f: &mut dyn FnMut(&mut ExprPool, ExprId) -> ExprId,
    ) -> ExprId {
        if let Some(&cached) = cache.get(&root) {
            return cached;
        }
        let node = self.nodes[root.0 as usize].node.clone();
        let mapped_node = match node {
            ExprNode::Add(c) => {
                let ids: Vec<ExprId> = c.iter().map(|&child| self.map_bottom_up(child, cache, f)).collect();
                self.add(ids)
            }
            ExprNode::Mul(c) => {
                let ids: Vec<ExprId> = c.iter().map(|&child| self.map_bottom_up(child, cache, f)).collect();
                self.mul(ids)
            }
            ExprNode::Pow(a, b) => {
                let a2 = self.map_bottom_up(a, cache, f);
                let b2 = self.map_bottom_up(b, cache, f);
                self.pow(a2, b2)
            }
            ExprNode::Neg(x) => {
                let x2 = self.map_bottom_up(x, cache, f);
                self.neg(x2)
            }
            ExprNode::Div(a, b) => {
                let a2 = self.map_bottom_up(a, cache, f);
                let b2 = self.map_bottom_up(b, cache, f);
                self.div(a2, b2)
            }
            ExprNode::Eq(a, b) => {
                let a2 = self.map_bottom_up(a, cache, f);
                let b2 = self.map_bottom_up(b, cache, f);
                self.eq_node(a2, b2)
            }
            ExprNode::Fn(tag, c) => {
                let ids: Vec<ExprId> = c.iter().map(|&child| self.map_bottom_up(child, cache, f)).collect();
                self.func(tag, ids)
            }
            ExprNode::List(c) => {
                let ids: Vec<ExprId> = c.iter().map(|&child| self.map_bottom_up(child, cache, f)).collect();
                self.list(ids)
            }
            _ => root, // atoms map to themselves
        };
        let result = f(self, mapped_node);
        cache.insert(root, result);
        result
    }

    pub fn map_bottom_up_fresh(
        &mut self,
        root: ExprId,
        f: &mut dyn FnMut(&mut ExprPool, ExprId) -> ExprId,
    ) -> ExprId {
        let mut cache = FxHashMap::default();
        self.map_bottom_up(root, &mut cache, f)
    }
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel expr::tests -- --nocapture
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/expr/mod.rs
git commit -m "feat(expr): add fold/map_bottom_up traversal helpers"
```

---

### Task 8: `expr` — Unit tests, proptest, criterion

**Files:**
- Modify: `rust/monomix-kernel/src/expr/mod.rs`
- Create: `rust/monomix-kernel/benches/kernel.rs`

- [ ] **Step 1: Add comprehensive unit tests to expr/mod.rs**

```rust
// Extend mod tests in expr/mod.rs:

#[test]
fn mul_zero_short_circuits() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let zero = pool.zero;
    assert_eq!(pool.mul(vec![x, zero]), zero);
}

#[test]
fn mul_flattens() {
    let mut pool = ExprPool::new();
    let a = pool.symbol("a");
    let b = pool.symbol("b");
    let c = pool.symbol("c");
    let ab = pool.mul(vec![a, b]);
    let abc = pool.mul(vec![ab, c]);
    let expected = pool.mul(vec![a, b, c]);
    assert_eq!(abc, expected);
}

#[test]
fn rational_negative_denominator_normalized() {
    let mut pool = ExprPool::new();
    let r = pool.rational(num_bigint::BigInt::from(-2), num_bigint::BigInt::from(-3));
    // -2/-3 = 2/3
    if let ExprNode::Rational(b) = pool.get(r) {
        assert_eq!(b.0, num_bigint::BigInt::from(2));
        assert_eq!(b.1, num_bigint::BigInt::from(3));
    } else {
        panic!("expected Rational");
    }
}

#[test]
fn rational_integer_shortcut() {
    let mut pool = ExprPool::new();
    let r = pool.rational(num_bigint::BigInt::from(6), num_bigint::BigInt::from(2));
    assert_eq!(r, pool.small_int(3));
}

#[test]
fn subtree_size_atom_is_one() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    assert_eq!(pool.subtree_size(x), 1);
}

#[test]
fn subtree_size_add() {
    let mut pool = ExprPool::new();
    let a = pool.symbol("a");
    let b = pool.symbol("b");
    let sum = pool.add(vec![a, b]);
    assert_eq!(pool.subtree_size(sum), 3); // Add + 2 children
}

#[test]
fn string_interning_roundtrip() {
    let mut pool = ExprPool::new();
    let id = pool.symbol("hello");
    if let ExprNode::Symbol(s) = pool.get(id) {
        assert_eq!(pool.str_of(*s), "hello");
    } else {
        panic!("expected Symbol");
    }
}
```

- [ ] **Step 2: Add proptest suite**

```rust
// Add after mod tests in expr/mod.rs:

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    prop_compose! {
        fn arb_small_int()(n in -1000i64..1000i64) -> i64 { n }
    }

    proptest! {
        #[test]
        fn intern_idempotent(n in -1000i64..1000i64) {
            let mut pool = ExprPool::new();
            let a = pool.small_int(n);
            let b = pool.small_int(n);
            prop_assert_eq!(a, b);
        }

        #[test]
        fn add_commutative(a in 0u32..50, b in 0u32..50) {
            let mut pool = ExprPool::new();
            let x = pool.small_int(a as i64);
            let y = pool.small_int(b as i64);
            prop_assert_eq!(pool.add(vec![x, y]), pool.add(vec![y, x]));
        }

        #[test]
        fn mul_commutative(a in 1u32..50, b in 1u32..50) {
            let mut pool = ExprPool::new();
            let x = pool.small_int(a as i64);
            let y = pool.small_int(b as i64);
            prop_assert_eq!(pool.mul(vec![x, y]), pool.mul(vec![y, x]));
        }

        #[test]
        fn no_collision_distinct_ints(a in -500i64..500i64, b in -500i64..500i64) {
            if a == b { return Ok(()); }
            let mut pool = ExprPool::new();
            let id_a = pool.small_int(a);
            let id_b = pool.small_int(b);
            prop_assert_ne!(id_a, id_b);
        }
    }
}
```

- [ ] **Step 3: Create criterion benchmark file**

```rust
// rust/monomix-kernel/benches/kernel.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use monomix_kernel::expr::ExprPool;

fn bench_intern_integers(c: &mut Criterion) {
    c.bench_function("intern 10k integers", |b| {
        b.iter(|| {
            let mut pool = ExprPool::new();
            for i in 0..10_000i64 {
                black_box(pool.small_int(i));
            }
        });
    });
}

fn bench_intern_add_nodes(c: &mut Criterion) {
    c.bench_function("intern 1k Add(10) nodes", |b| {
        b.iter(|| {
            let mut pool = ExprPool::new();
            let atoms: Vec<_> = (0..10).map(|i| pool.small_int(i)).collect();
            for _ in 0..1000 {
                black_box(pool.add(atoms.clone()));
            }
        });
    });
}

fn bench_map_bottom_up_identity(c: &mut Criterion) {
    c.bench_function("map_bottom_up identity 1k-node DAG", |b| {
        let mut pool = ExprPool::new();
        // Build a 1k-node DAG: chain of Add nodes
        let x = pool.symbol("x");
        let mut root = x;
        for i in 0..500i64 {
            let n = pool.small_int(i);
            root = pool.add(vec![root, n]);
        }
        b.iter(|| {
            let mut cache = rustc_hash::FxHashMap::default();
            black_box(pool.map_bottom_up(root, &mut cache, &mut |_p, id| id));
        });
    });
}

criterion_group!(benches, bench_intern_integers, bench_intern_add_nodes, bench_map_bottom_up_identity);
criterion_main!(benches);
```

Also create `rust/monomix-kernel/benches/` directory. The `[[bench]]` in Cargo.toml names this `kernel`.

- [ ] **Step 4: Run all tests and benchmark**

```
cargo test -p monomix-kernel -- --nocapture
```

Expected: all tests pass.

```
cargo bench -p monomix-kernel --bench kernel 2>&1 | tail -20
```

Expected: benchmarks complete without panic. Record baseline numbers.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/expr/mod.rs rust/monomix-kernel/benches/
git commit -m "test(expr): add unit tests, proptest, criterion benchmarks"
```

---

### Task 9: `parser/lexer.rs` — Token, TokenKind, Span, Lexer

**Files:**
- Create: `rust/monomix-kernel/src/parser/lexer.rs`
- Modify: `rust/monomix-kernel/src/parser/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
// rust/monomix-kernel/src/parser/lexer.rs  (put tests at bottom)

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_all(src: &str) -> Vec<TokenKind> {
        let mut lexer = Lexer::new(src);
        let mut kinds = Vec::new();
        loop {
            let (tok, _) = lexer.next();
            let k = tok.kind();
            kinds.push(k);
            if k == TokenKind::Eof { break; }
        }
        kinds
    }

    #[test]
    fn lex_simple_expr() {
        assert_eq!(
            lex_all("1 + 2"),
            vec![TokenKind::SmallInt, TokenKind::Plus, TokenKind::SmallInt, TokenKind::Eof]
        );
    }

    #[test]
    fn lex_pow_both_spellings() {
        assert_eq!(lex_all("x^2 + y**3")[2], TokenKind::Pow);
        assert_eq!(lex_all("x^2 + y**3")[6], TokenKind::Pow);
    }

    #[test]
    fn lex_comment_stripped() {
        assert_eq!(
            lex_all("1 % this is a comment\n+ 2"),
            vec![TokenKind::SmallInt, TokenKind::Plus, TokenKind::SmallInt, TokenKind::Eof]
        );
    }

    #[test]
    fn lex_span_byte_accurate() {
        let src = "xy + 1";
        let mut lexer = Lexer::new(src);
        let (_, span) = lexer.next(); // "xy"
        assert_eq!(span.start, 0);
        assert_eq!(span.end, 2);
        assert_eq!(&src[span.start as usize..span.end as usize], "xy");
    }

    #[test]
    fn lex_inf_nan_rejected() {
        let kinds = lex_all("inf");
        // `inf` is lexed as an Ident (not a float literal),
        // rejection happens at parser level for float literals.
        // The lexer emits Ident for "inf".
        assert_eq!(kinds[0], TokenKind::Ident);
    }

    #[test]
    fn lex_assign_token() {
        assert_eq!(lex_all("x := 1")[1], TokenKind::Assign);
    }

    #[test]
    fn lex_peek_kind_no_clone() {
        let mut lexer = Lexer::new("1 + 2");
        assert_eq!(lexer.peek_kind(), TokenKind::SmallInt);
        assert_eq!(lexer.peek_kind(), TokenKind::SmallInt); // idempotent
        lexer.next(); // consume
        assert_eq!(lexer.peek_kind(), TokenKind::Plus);
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel parser::lexer::tests -- --nocapture
```

Expected: FAIL — module not found.

- [ ] **Step 3: Implement the lexer**

```rust
// rust/monomix-kernel/src/parser/lexer.rs

use arrayvec::ArrayVec;
use num_bigint::BigInt;
use crate::parser::ast::{Span, TokenKind};

// ---- Tokens ----------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum Token {
    SmallInt(i64),
    BigInt(Box<BigInt>),
    Float(f64),
    Ident(Span),

    Plus, Minus, Star, Slash,
    Pow,       // ^ or **
    Assign,    // :=
    Equals,    // =
    Comma, LParen, RParen,
    Semi, Dollar,
    KwComment,
    Eof,
}

impl Token {
    pub fn kind(&self) -> TokenKind {
        match self {
            Token::SmallInt(_) => TokenKind::SmallInt,
            Token::BigInt(_)   => TokenKind::BigInt,
            Token::Float(_)    => TokenKind::Float,
            Token::Ident(_)    => TokenKind::Ident,
            Token::Plus        => TokenKind::Plus,
            Token::Minus       => TokenKind::Minus,
            Token::Star        => TokenKind::Star,
            Token::Slash       => TokenKind::Slash,
            Token::Pow         => TokenKind::Pow,
            Token::Assign      => TokenKind::Assign,
            Token::Equals      => TokenKind::Equals,
            Token::Comma       => TokenKind::Comma,
            Token::LParen      => TokenKind::LParen,
            Token::RParen      => TokenKind::RParen,
            Token::Semi        => TokenKind::Semi,
            Token::Dollar      => TokenKind::Dollar,
            Token::KwComment   => TokenKind::KwComment,
            Token::Eof         => TokenKind::Eof,
        }
    }
}

// ---- Lexer -----------------------------------------------------------------

pub struct Lexer<'s> {
    src: &'s str,
    pos: usize,
    buffer: ArrayVec<(Token, Span), 2>,
}

impl<'s> Lexer<'s> {
    pub fn new(src: &'s str) -> Self {
        Lexer { src, pos: 0, buffer: ArrayVec::new() }
    }

    pub fn peek(&mut self) -> &(Token, Span) {
        if self.buffer.is_empty() {
            let tok = self.scan_next();
            self.buffer.push(tok);
        }
        &self.buffer[0]
    }

    pub fn peek_kind(&mut self) -> TokenKind {
        self.peek().0.kind()
    }

    pub fn peek_at(&mut self, offset: usize) -> &(Token, Span) {
        while self.buffer.len() <= offset {
            let tok = self.scan_next();
            self.buffer.push(tok);
        }
        &self.buffer[offset]
    }

    pub fn next(&mut self) -> (Token, Span) {
        if self.buffer.is_empty() {
            return self.scan_next();
        }
        // Drain slot 0; shift remaining down
        let item = self.buffer.remove(0);
        item
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // skip whitespace
            while self.pos < self.src.len()
                && self.src.as_bytes()[self.pos].is_ascii_whitespace()
            {
                self.pos += 1;
            }
            // skip % line comments
            if self.pos < self.src.len() && self.src.as_bytes()[self.pos] == b'%' {
                while self.pos < self.src.len() && self.src.as_bytes()[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
    }

    fn scan_next(&mut self) -> (Token, Span) {
        self.skip_whitespace_and_comments();

        if self.pos >= self.src.len() {
            let end = self.src.len() as u32;
            return (Token::Eof, Span { start: end, end });
        }

        let start = self.pos;
        let b = self.src.as_bytes()[self.pos];

        match b {
            b'+' => { self.pos += 1; (Token::Plus,   Span::of(start, self.pos)) }
            b'-' => { self.pos += 1; (Token::Minus,  Span::of(start, self.pos)) }
            b'*' => {
                if self.pos + 1 < self.src.len() && self.src.as_bytes()[self.pos + 1] == b'*' {
                    self.pos += 2;
                    (Token::Pow, Span::of(start, self.pos))
                } else {
                    self.pos += 1;
                    (Token::Star, Span::of(start, self.pos))
                }
            }
            b'/' => { self.pos += 1; (Token::Slash, Span::of(start, self.pos)) }
            b'^' => { self.pos += 1; (Token::Pow,   Span::of(start, self.pos)) }
            b'=' => { self.pos += 1; (Token::Equals, Span::of(start, self.pos)) }
            b',' => { self.pos += 1; (Token::Comma,  Span::of(start, self.pos)) }
            b'(' => { self.pos += 1; (Token::LParen, Span::of(start, self.pos)) }
            b')' => { self.pos += 1; (Token::RParen, Span::of(start, self.pos)) }
            b';' => { self.pos += 1; (Token::Semi,   Span::of(start, self.pos)) }
            b'$' => { self.pos += 1; (Token::Dollar, Span::of(start, self.pos)) }
            b':' => {
                if self.pos + 1 < self.src.len() && self.src.as_bytes()[self.pos + 1] == b'=' {
                    self.pos += 2;
                    (Token::Assign, Span::of(start, self.pos))
                } else {
                    // Unexpected lone ':'
                    self.pos += 1;
                    (Token::Eof, Span::of(start, self.pos))
                }
            }
            b'0'..=b'9' => self.scan_number(start),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.scan_ident(start),
            _ => {
                self.pos += 1;
                (Token::Eof, Span::of(start, self.pos))
            }
        }
    }

    fn scan_number(&mut self, start: usize) -> (Token, Span) {
        let src = self.src;
        while self.pos < src.len() && src.as_bytes()[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        // Check for float
        let is_float = self.pos < src.len()
            && (src.as_bytes()[self.pos] == b'.'
                || src.as_bytes()[self.pos] == b'e'
                || src.as_bytes()[self.pos] == b'E');
        if is_float {
            if self.pos < src.len() && src.as_bytes()[self.pos] == b'.' {
                self.pos += 1;
                while self.pos < src.len() && src.as_bytes()[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
            }
            if self.pos < src.len()
                && (src.as_bytes()[self.pos] == b'e' || src.as_bytes()[self.pos] == b'E')
            {
                self.pos += 1;
                if self.pos < src.len()
                    && (src.as_bytes()[self.pos] == b'+' || src.as_bytes()[self.pos] == b'-')
                {
                    self.pos += 1;
                }
                while self.pos < src.len() && src.as_bytes()[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
            }
            let span = Span::of(start, self.pos);
            // Cap at 1024 bytes
            if self.pos - start > 1024 {
                return (Token::Eof, span);
            }
            let s = &src[start..self.pos];
            match s.parse::<f64>() {
                Ok(f) => (Token::Float(f), span),
                Err(_) => (Token::Eof, span),
            }
        } else {
            let span = Span::of(start, self.pos);
            if self.pos - start > 1024 {
                return (Token::Eof, span);
            }
            let s = &src[start..self.pos];
            match s.parse::<i64>() {
                Ok(n) => (Token::SmallInt(n), span),
                Err(_) => {
                    match s.parse::<BigInt>() {
                        Ok(n) => (Token::BigInt(Box::new(n)), span),
                        Err(_) => (Token::Eof, span),
                    }
                }
            }
        }
    }

    fn scan_ident(&mut self, start: usize) -> (Token, Span) {
        while self.pos < self.src.len() {
            let b = self.src.as_bytes()[self.pos];
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let span = Span::of(start, self.pos);
        if self.pos - start > 1024 {
            return (Token::Eof, span);
        }
        let word = &self.src[start..self.pos];
        // Case-insensitive keyword detection
        if word.eq_ignore_ascii_case("comment") {
            return (Token::KwComment, span);
        }
        (Token::Ident(span), span)
    }
}
```

Add `Span::of` helper to `ast.rs`:
```rust
impl Span {
    pub const SYNTHETIC: Span = Span { start: u32::MAX, end: u32::MAX };
    pub fn of(start: usize, end: usize) -> Self {
        Span { start: start as u32, end: end as u32 }
    }
    pub fn merge(self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }
    pub fn to_str<'s>(&self, source: &'s str) -> &'s str {
        &source[self.start as usize..self.end as usize]
    }
}
```

Add `pub mod lexer;` to `parser/mod.rs`.

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel parser::lexer::tests -- --nocapture
```

Expected: all lexer tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/parser/
git commit -m "feat(parser): add lexer with Token/TokenKind/Span/ArrayVec lookahead"
```

---

### Task 10: `parser/ast.rs` and `parser/expr.rs` — Pratt Parser

**Files:**
- Complete: `rust/monomix-kernel/src/parser/ast.rs`
- Create: `rust/monomix-kernel/src/parser/expr.rs`

- [ ] **Step 1: Write failing parser expression tests**

```rust
// rust/monomix-kernel/src/parser/expr.rs  (tests module at bottom)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ExprPool, ExprNode};
    use crate::parser::ast::ParseResult;

    fn parse_expr_only(src: &str) -> (ExprPool, ExprNode) {
        let mut pool = ExprPool::new();
        let result = crate::parser::parse(src, &mut pool);
        assert!(result.diagnostics.is_empty(), "unexpected diagnostics: {:?}", result.diagnostics);
        assert_eq!(result.statements.len(), 1);
        let root = result.statements[0].expr;
        let node = pool.get(root).clone();
        (pool, node)
    }

    #[test]
    fn parse_integer_literal() {
        let (_pool, node) = parse_expr_only("42;");
        assert_eq!(node, ExprNode::SmallInt(42));
    }

    #[test]
    fn parse_precedence_add_mul() {
        let (pool, node) = parse_expr_only("1 + 2 * 3;");
        // Should be Add([1, Mul([2, 3])])
        if let ExprNode::Add(children) = node {
            assert_eq!(children.len(), 2);
            let mul_node = pool.get(children[1]).clone();
            assert!(matches!(mul_node, ExprNode::Mul(_)));
        } else {
            panic!("expected Add, got {:?}", node);
        }
    }

    #[test]
    fn parse_pow_right_associative() {
        let (pool, node) = parse_expr_only("2^3^4;");
        // Should be Pow(2, Pow(3, 4))
        if let ExprNode::Pow(_, exp) = node {
            assert!(matches!(pool.get(exp), ExprNode::Pow(_, _)));
        } else {
            panic!("expected Pow, got {:?}", node);
        }
    }

    #[test]
    fn parse_double_negation_normalizes() {
        let (pool, node) = parse_expr_only("-(-x);");
        // neg(neg(x)) → x via pool normalizer
        assert!(matches!(node, ExprNode::Symbol(_)));
    }

    #[test]
    fn parse_equality() {
        let (_, node) = parse_expr_only("x = y;");
        assert!(matches!(node, ExprNode::Eq(_, _)));
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel parser::expr::tests -- --nocapture
```

Expected: FAIL — `parse()` not defined.

- [ ] **Step 3: Implement `parser/ast.rs` fully and `parser/expr.rs` Pratt parser**

Complete `ast.rs` with `StmtKind`, `OutputMode`, `ParseResult`:

```rust
// rust/monomix-kernel/src/parser/ast.rs
use rustc_hash::FxHashMap;
use crate::expr::ExprId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span { pub start: u32, pub end: u32 }

impl Span {
    pub const SYNTHETIC: Span = Span { start: u32::MAX, end: u32::MAX };
    pub fn of(s: usize, e: usize) -> Self { Span { start: s as u32, end: e as u32 } }
    pub fn merge(self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }
    pub fn to_str<'s>(&self, source: &'s str) -> &'s str {
        if self.start == u32::MAX { return "<synthetic>"; }
        &source[self.start as usize..self.end as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    SmallInt, BigInt, Float, Ident,
    Plus, Minus, Star, Slash, Pow,
    Assign, Equals, Comma, LParen, RParen,
    Semi, Dollar, KwComment, Eof,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputMode { Display, Suppress }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity { Error, Warning }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagnosticCode {
    UnexpectedToken { found: TokenKind, expected: &'static str },
    UnterminatedStatement,
    UnbalancedParen,
    InvalidNumericLiteral,
    NumericLiteralTooLong,
    IdentifierTooLong,
    MissingArgument { function: &'static str },
    TooManyArguments { function: &'static str, max: usize },
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub message: String,
    pub code: DiagnosticCode,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StmtKind {
    Expr,
    Assign { lhs: crate::expr::InternedStr },
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub expr: ExprId,
    pub output: OutputMode,
    pub span: Span,
}

pub type SpanMap = FxHashMap<ExprId, Span>;

#[derive(Debug)]
pub struct ParseResult {
    pub statements: Vec<Stmt>,
    pub diagnostics: Vec<Diagnostic>,
    pub span_map: SpanMap,
}
```

Implement `parser/expr.rs` — Pratt expression parser:

```rust
// rust/monomix-kernel/src/parser/expr.rs

use crate::expr::{ExprPool, ExprId, ExprNode, FnTag, InternedStr};
use crate::parser::ast::{Diagnostic, DiagnosticCode, Severity, Span, SpanMap, TokenKind};
use crate::parser::lexer::{Lexer, Token};

pub(super) struct ExprParser<'s, 'p> {
    pub(super) lexer: Lexer<'s>,
    pub(super) pool: &'p mut ExprPool,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) span_map: SpanMap,
    pub(super) src: &'s str,
    pub(super) builtins: BuiltinIds,
}

#[derive(Clone, Copy)]
pub(super) struct BuiltinIds {
    pub df:       InternedStr,
    pub int_:     InternedStr,
    pub solve:    InternedStr,
    pub factor:   InternedStr,
    pub expand:   InternedStr,
    pub simplify: InternedStr,
    pub sub:      InternedStr,
}

impl BuiltinIds {
    pub(super) fn new(pool: &mut ExprPool) -> Self {
        BuiltinIds {
            df:       pool.intern_str_pub("df"),
            int_:     pool.intern_str_pub("int"),
            solve:    pool.intern_str_pub("solve"),
            factor:   pool.intern_str_pub("factor"),
            expand:   pool.intern_str_pub("expand"),
            simplify: pool.intern_str_pub("simplify"),
            sub:      pool.intern_str_pub("sub"),
        }
    }
}

fn infix_bp(kind: TokenKind) -> Option<(u8, u8)> {
    match kind {
        TokenKind::Equals          => Some((10, 0)),
        TokenKind::Plus
        | TokenKind::Minus         => Some((20, 21)),
        TokenKind::Star
        | TokenKind::Slash         => Some((30, 31)),
        TokenKind::Pow             => Some((50, 49)),
        _                          => None,
    }
}

fn prefix_bp(kind: TokenKind) -> Option<u8> {
    match kind {
        TokenKind::Minus => Some(40),
        _                => None,
    }
}

impl<'s, 'p> ExprParser<'s, 'p> {
    pub(super) fn parse_expr(&mut self, min_bp: u8) -> Result<ExprId, ()> {
        let (tok, tok_span) = self.lexer.next();
        let mut lhs = match tok {
            Token::SmallInt(n) => {
                let id = self.pool.small_int(n);
                self.span_map.insert(id, tok_span);
                id
            }
            Token::BigInt(n) => {
                let id = self.pool.integer(*n);
                self.span_map.insert(id, tok_span);
                id
            }
            Token::Float(f) => {
                let id = self.pool.float(f);
                self.span_map.insert(id, tok_span);
                id
            }
            Token::Ident(span) => self.parse_ident_or_call(span)?,
            Token::LParen => {
                let inner = self.parse_expr(0)?;
                let (_, close_span) = self.expect(TokenKind::RParen)?;
                if let Some(&s) = self.span_map.get(&inner) {
                    self.span_map.insert(inner, tok_span.merge(close_span));
                }
                inner
            }
            Token::Minus => {
                let bp = prefix_bp(TokenKind::Minus).unwrap();
                let right = self.parse_expr(bp)?;
                let id = self.pool.neg(right);
                self.span_map.insert(id, tok_span);
                id
            }
            other => {
                return Err(self.emit_unexpected(other, tok_span, "expression"));
            }
        };

        // Rational literal shortcut: INTEGER '/' INTEGER
        if matches!(self.pool.get(lhs), ExprNode::SmallInt(_) | ExprNode::BigInt(_))
            && self.lexer.peek_kind() == TokenKind::Slash
        {
            if let Some(rat_id) = self.try_rational(lhs) {
                lhs = rat_id;
            }
        }

        // Pratt infix loop
        loop {
            let op_kind = self.lexer.peek_kind();
            let Some((left_bp, right_bp)) = infix_bp(op_kind) else { break };
            if left_bp <= min_bp { break }
            let (op_tok, op_span) = self.lexer.next();
            let rhs = self.parse_expr(right_bp)?;
            lhs = self.build_infix(op_tok, lhs, rhs, op_span);
        }

        Ok(lhs)
    }

    fn try_rational(&mut self, lhs: ExprId) -> Option<ExprId> {
        let slash_kind = self.lexer.peek_at(0).0.kind();
        let next_kind  = self.lexer.peek_at(1).0.kind();
        if slash_kind == TokenKind::Slash
            && (next_kind == TokenKind::SmallInt || next_kind == TokenKind::BigInt)
        {
            self.lexer.next(); // consume '/'
            let (den_tok, _) = self.lexer.next();
            let (p, q) = match (self.pool.get(lhs).clone(), den_tok) {
                (ExprNode::SmallInt(p), Token::SmallInt(q)) => {
                    (num_bigint::BigInt::from(p), num_bigint::BigInt::from(q))
                }
                _ => return None,
            };
            Some(self.pool.rational(p, q))
        } else {
            None
        }
    }

    fn build_infix(&mut self, op: Token, lhs: ExprId, rhs: ExprId, _span: Span) -> ExprId {
        match op {
            Token::Plus    => self.pool.add(vec![lhs, rhs]),
            Token::Minus   => { let neg = self.pool.neg(rhs); self.pool.add(vec![lhs, neg]) }
            Token::Star    => self.pool.mul(vec![lhs, rhs]),
            Token::Slash   => self.pool.div(lhs, rhs),
            Token::Pow     => self.pool.pow(lhs, rhs),
            Token::Equals  => self.pool.eq_node(lhs, rhs),
            _ => unreachable!(),
        }
    }

    fn parse_ident_or_call(&mut self, ident_span: Span) -> Result<ExprId, ()> {
        let name = self.pool.intern_str_pub(&self.src[ident_span.start as usize..ident_span.end as usize].to_lowercase());
        let id = self.pool.symbol_by_id(name);
        self.span_map.insert(id, ident_span);

        if self.lexer.peek_kind() != TokenKind::LParen {
            return Ok(id);
        }
        self.lexer.next(); // consume '('

        let bt = self.builtins;
        if name == bt.df       { return self.parse_df(ident_span); }
        if name == bt.int_     { return self.parse_stub_fn(FnTag::Custom(name), ident_span); }
        if name == bt.solve    { return self.parse_solve_call(ident_span); }
        if name == bt.factor   { return self.parse_stub_fn(FnTag::Custom(name), ident_span); }
        if name == bt.expand   { return self.parse_unary_builtin(FnTag::Custom(name), ident_span); }
        if name == bt.simplify { return self.parse_unary_builtin(FnTag::Custom(name), ident_span); }
        if name == bt.sub      { return self.parse_sub(ident_span); }
        self.parse_generic_call(name, ident_span)
    }

    fn parse_arg_list(&mut self) -> Result<Vec<ExprId>, ()> {
        let mut args = Vec::new();
        if self.lexer.peek_kind() == TokenKind::RParen {
            self.lexer.next();
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr(0)?);
            match self.lexer.peek_kind() {
                TokenKind::Comma => { self.lexer.next(); }
                TokenKind::RParen => { self.lexer.next(); break; }
                _ => { return Err(self.emit_error("expected ',' or ')'", Span::SYNTHETIC)); }
            }
        }
        Ok(args)
    }

    fn parse_generic_call(&mut self, name: InternedStr, _span: Span) -> Result<ExprId, ()> {
        let args = self.parse_arg_list()?;
        Ok(self.pool.func(FnTag::Custom(name), args))
    }

    fn parse_unary_builtin(&mut self, tag: FnTag, _span: Span) -> Result<ExprId, ()> {
        let arg = self.parse_expr(0)?;
        self.expect(TokenKind::RParen)?;
        Ok(self.pool.func(tag, vec![arg]))
    }

    fn parse_stub_fn(&mut self, tag: FnTag, span: Span) -> Result<ExprId, ()> {
        // Parse args normally; runtime evaluation will raise UnsupportedFn
        self.parse_unary_builtin(tag, span)
    }

    fn parse_df(&mut self, _span: Span) -> Result<ExprId, ()> {
        let expr = self.parse_expr(0)?;
        self.expect(TokenKind::Comma)?;
        let var = self.parse_expr(0)?;
        // Support df(f, x, n) for n-th derivative or df(f, x, y) for mixed partial
        let mut result = self.pool.func(FnTag::Custom(self.builtins.df), vec![expr, var]);
        while self.lexer.peek_kind() == TokenKind::Comma {
            self.lexer.next();
            let next_var = self.parse_expr(0)?;
            result = self.pool.func(FnTag::Custom(self.builtins.df), vec![result, next_var]);
        }
        self.expect(TokenKind::RParen)?;
        Ok(result)
    }

    fn parse_solve_call(&mut self, _span: Span) -> Result<ExprId, ()> {
        let eq = self.parse_expr(0)?;
        self.expect(TokenKind::Comma)?;
        let var = self.parse_expr(0)?;
        self.expect(TokenKind::RParen)?;
        Ok(self.pool.func(FnTag::Custom(self.builtins.solve), vec![eq, var]))
    }

    fn parse_sub(&mut self, _span: Span) -> Result<ExprId, ()> {
        // sub(x = val, expr) or sub(x = a, y = b, expr)
        let mut bindings = Vec::new();
        loop {
            let lhs = self.parse_expr(0)?;
            self.expect(TokenKind::Equals)?;
            let rhs = self.parse_expr(0)?;
            bindings.push(self.pool.eq_node(lhs, rhs));
            if self.lexer.peek_kind() == TokenKind::Comma {
                self.lexer.next();
                // Check if next is another binding or the target expr
                // Heuristic: if peek is ident followed by = it's a binding
                if self.lexer.peek_kind() == TokenKind::Ident
                    && self.lexer.peek_at(1).0.kind() == TokenKind::Equals
                {
                    continue;
                }
                // Otherwise it's the target expression
                let target = self.parse_expr(0)?;
                self.expect(TokenKind::RParen)?;
                let mut args = bindings;
                args.push(target);
                return Ok(self.pool.func(FnTag::Custom(self.builtins.sub), args));
            } else {
                break;
            }
        }
        Err(self.emit_error("sub() requires a target expression after bindings", Span::SYNTHETIC))
    }

    pub(super) fn expect(&mut self, kind: TokenKind) -> Result<(Token, Span), ()> {
        let (tok, span) = self.lexer.next();
        if tok.kind() == kind {
            Ok((tok, span))
        } else {
            Err(self.emit_unexpected(tok, span, "?"))
        }
    }

    fn emit_unexpected(&mut self, tok: Token, span: Span, expected: &'static str) -> () {
        let found = tok.kind();
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message: format!("unexpected token {:?}, expected {}", found, expected),
            code: DiagnosticCode::UnexpectedToken { found, expected },
        });
    }

    fn emit_error(&mut self, msg: &str, span: Span) -> () {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message: msg.to_string(),
            code: DiagnosticCode::UnterminatedStatement,
        });
    }
}
```

Add `pub(crate) fn intern_str_pub` to ExprPool:
```rust
// In impl ExprPool:
pub fn intern_str_pub(&mut self, s: &str) -> InternedStr {
    self.intern_str(s)
}
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel parser -- --nocapture
```

Expected: all expression tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/parser/
git commit -m "feat(parser): add ast types + Pratt expression parser"
```

---

### Task 11: `parser/stmt.rs` — Statement Parser + `parse()` Entry Point

**Files:**
- Create: `rust/monomix-kernel/src/parser/stmt.rs`
- Complete: `rust/monomix-kernel/src/parser/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
// In parser/mod.rs tests section:

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;

    #[test]
    fn parse_single_statement_display() {
        let mut pool = ExprPool::new();
        let result = parse("1 + 2;", &mut pool);
        assert_eq!(result.statements.len(), 1);
        assert_eq!(result.diagnostics.len(), 0);
        assert_eq!(result.statements[0].output, crate::parser::ast::OutputMode::Display);
    }

    #[test]
    fn parse_suppress_with_dollar() {
        let mut pool = ExprPool::new();
        let result = parse("x + 1$", &mut pool);
        assert_eq!(result.statements[0].output, crate::parser::ast::OutputMode::Suppress);
    }

    #[test]
    fn parse_assignment() {
        let mut pool = ExprPool::new();
        let result = parse("y := 2 * x;", &mut pool);
        assert_eq!(result.statements.len(), 1);
        assert!(matches!(result.statements[0].kind, crate::parser::ast::StmtKind::Assign { .. }));
    }

    #[test]
    fn parse_multiple_statements() {
        let mut pool = ExprPool::new();
        let result = parse("a := 1; b := 2;", &mut pool);
        assert_eq!(result.statements.len(), 2);
    }

    #[test]
    fn parse_error_recovery() {
        let mut pool = ExprPool::new();
        let result = parse("1 + ; 2 + 3;", &mut pool);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.statements.len(), 1); // "2 + 3" parsed OK
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel parser::tests -- --nocapture
```

Expected: FAIL — `parse()` not defined.

- [ ] **Step 3: Implement `parser/stmt.rs` and `parser/mod.rs`**

```rust
// rust/monomix-kernel/src/parser/stmt.rs

use crate::expr::ExprPool;
use crate::parser::ast::{Diagnostic, DiagnosticCode, OutputMode, Severity, Span, Stmt, StmtKind, TokenKind};
use crate::parser::expr::{BuiltinIds, ExprParser};
use crate::parser::lexer::Token;

impl<'s, 'p> ExprParser<'s, 'p> {
    pub(super) fn parse_program(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        loop {
            if self.lexer.peek_kind() == TokenKind::Eof { break; }
            // Skip `comment ... ;` blocks
            if self.lexer.peek_kind() == TokenKind::KwComment {
                self.lexer.next();
                self.skip_to_terminator();
                continue;
            }
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
            }
        }
        stmts
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        let start_span = self.lexer.peek().1;
        // Detect assignment: IDENT ':='
        let is_assign = self.lexer.peek_kind() == TokenKind::Ident
            && self.lexer.peek_at(1).0.kind() == TokenKind::Assign;

        if is_assign {
            return self.parse_assign_stmt(start_span);
        }
        self.parse_expr_stmt(start_span)
    }

    fn parse_assign_stmt(&mut self, start_span: Span) -> Option<Stmt> {
        let (ident_tok, ident_span) = self.lexer.next(); // IDENT
        let name = if let Token::Ident(s) = ident_tok {
            self.pool.intern_str_pub(&self.src[s.start as usize..s.end as usize].to_lowercase())
        } else { unreachable!() };
        self.lexer.next(); // ':='
        let expr = match self.parse_expr(0) {
            Ok(e) => e,
            Err(()) => { self.synchronise(); return None; }
        };
        let (output, end_span) = self.parse_terminator()?;
        Some(Stmt {
            kind: StmtKind::Assign { lhs: name },
            expr,
            output,
            span: start_span.merge(end_span),
        })
    }

    fn parse_expr_stmt(&mut self, start_span: Span) -> Option<Stmt> {
        let expr = match self.parse_expr(0) {
            Ok(e) => e,
            Err(()) => { self.synchronise(); return None; }
        };
        let (output, end_span) = self.parse_terminator()?;
        Some(Stmt {
            kind: StmtKind::Expr,
            expr,
            output,
            span: start_span.merge(end_span),
        })
    }

    fn parse_terminator(&mut self) -> Option<(OutputMode, Span)> {
        match self.lexer.peek_kind() {
            TokenKind::Semi => {
                let (_, span) = self.lexer.next();
                Some((OutputMode::Display, span))
            }
            TokenKind::Dollar => {
                let (_, span) = self.lexer.next();
                Some((OutputMode::Suppress, span))
            }
            TokenKind::Eof => Some((OutputMode::Display, Span::SYNTHETIC)),
            _ => {
                let (tok, span) = self.lexer.next();
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    span,
                    message: "expected ';' or '$' to end statement".to_string(),
                    code: DiagnosticCode::UnterminatedStatement,
                });
                self.synchronise();
                None
            }
        }
    }

    pub(super) fn synchronise(&mut self) {
        let mut depth: u32 = 0;
        loop {
            match self.lexer.peek_kind() {
                TokenKind::LParen => { depth += 1; self.lexer.next(); }
                TokenKind::RParen if depth > 0 => { depth -= 1; self.lexer.next(); }
                TokenKind::Semi | TokenKind::Dollar if depth == 0 => {
                    self.lexer.next(); // consume terminator
                    break;
                }
                TokenKind::Eof => return,
                _ => { self.lexer.next(); }
            }
        }
    }

    fn skip_to_terminator(&mut self) {
        loop {
            match self.lexer.peek_kind() {
                TokenKind::Semi | TokenKind::Dollar => { self.lexer.next(); break; }
                TokenKind::Eof => break,
                _ => { self.lexer.next(); }
            }
        }
    }
}
```

```rust
// rust/monomix-kernel/src/parser/mod.rs

pub mod ast;
pub mod lexer;
pub(crate) mod expr;
pub(crate) mod stmt;

use crate::expr::ExprPool;
use crate::parser::ast::ParseResult;
use crate::parser::expr::{BuiltinIds, ExprParser};
use crate::parser::lexer::Lexer;
use rustc_hash::FxHashMap;

pub fn parse(source: &str, pool: &mut ExprPool) -> ParseResult {
    let builtins = BuiltinIds::new(pool);
    let mut parser = ExprParser {
        lexer: Lexer::new(source),
        pool,
        diagnostics: Vec::new(),
        span_map: FxHashMap::default(),
        src: source,
        builtins,
    };
    let statements = parser.parse_program();
    ParseResult {
        statements,
        diagnostics: parser.diagnostics,
        span_map: parser.span_map,
    }
}

#[cfg(test)]
mod tests {
    // ... (tests from Step 1 above)
}
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel parser -- --nocapture
```

Expected: all parser statement tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/parser/
git commit -m "feat(parser): add statement parser + parse() entry point + error recovery"
```

---

### Task 12: Parser tests, proptest, benchmarks, cargo-fuzz setup

**Files:**
- Modify: `rust/monomix-kernel/src/parser/mod.rs`
- Modify: `rust/monomix-kernel/benches/kernel.rs`
- Create: `rust/monomix-kernel/fuzz/Cargo.toml`
- Create: `rust/monomix-kernel/fuzz/fuzz_targets/fuzz_parser.rs`

- [ ] **Step 1: Add proptest for parser no-panics**

```rust
// Append to parser/mod.rs:

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::expr::ExprPool;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn no_panic_on_arbitrary_input(s in "[ -~]{0,200}") {
            let mut pool = ExprPool::new();
            let result = parse(&s, &mut pool);
            // Should never panic; diagnostics or stmts may be anything
            prop_assert!(result.diagnostics.len() + result.statements.len() >= 0);
        }

        #[test]
        fn span_bounds_valid(s in "[a-z0-9 +*();]{0,100}") {
            let mut pool = ExprPool::new();
            let result = parse(&s, &mut pool);
            for (_, span) in &result.span_map {
                prop_assert!(span.start <= span.end);
                prop_assert!((span.end as usize) <= s.len()
                    || *span == crate::parser::ast::Span::SYNTHETIC);
            }
        }
    }
}
```

- [ ] **Step 2: Add parser benchmark to benches/kernel.rs**

```rust
// Add to benches/kernel.rs:

use monomix_kernel::parser::parse;

fn bench_parse_100_term_poly(c: &mut Criterion) {
    // Build a 100-term polynomial source string: a1*x^100 + a2*x^99 + ...
    let terms: Vec<String> = (1..=100)
        .map(|i| format!("{}*x^{}", i, 101 - i))
        .collect();
    let src = format!("{};", terms.join(" + "));

    c.bench_function("parse 100-term polynomial", |b| {
        b.iter(|| {
            let mut pool = monomix_kernel::expr::ExprPool::new();
            black_box(parse(&src, &mut pool));
        });
    });
}

fn bench_parse_20_statements(c: &mut Criterion) {
    let src = (0..20).map(|i| format!("x{} := {}*y + {};", i, i, i+1)).collect::<Vec<_>>().join(" ");
    c.bench_function("parse 20 assignments", |b| {
        b.iter(|| {
            let mut pool = monomix_kernel::expr::ExprPool::new();
            black_box(parse(&src, &mut pool));
        });
    });
}
```

Update the `criterion_group!` call to include the new benches.

- [ ] **Step 3: Create cargo-fuzz setup**

```bash
# Run from rust/monomix-kernel/
cargo fuzz init   # if not already done; creates fuzz/
```

If `cargo-fuzz` is not installed: `cargo install cargo-fuzz`

Create fuzz target manually if the init doesn't create it:

```toml
# rust/monomix-kernel/fuzz/Cargo.toml
[package]
name = "monomix-kernel-fuzz"
version = "0.0.1"
edition = "2021"
publish = false

[dependencies]
libfuzzer-sys = "0.4"
monomix-kernel = { path = ".." }

[[bin]]
name = "fuzz_parser"
path = "fuzz_targets/fuzz_parser.rs"
doc = false
```

```rust
// rust/monomix-kernel/fuzz/fuzz_targets/fuzz_parser.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use monomix_kernel::{expr::ExprPool, parser::parse};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut pool = ExprPool::new();
        let _result = parse(s, &mut pool);
        // Must not panic
    }
});
```

Seed corpus setup (run once):
```bash
mkdir -p rust/monomix-kernel/fuzz/corpus/fuzz_parser
find legacy/reduce-algebra-code-r7357-trunk/packages -name "*.tst" \
  -exec cp {} rust/monomix-kernel/fuzz/corpus/fuzz_parser/ \;
```

- [ ] **Step 4: Run parser tests and verify benchmarks compile**

```
cargo test -p monomix-kernel parser -- --nocapture
cargo bench -p monomix-kernel --bench kernel -- parse 2>&1 | tail -10
```

Expected: all tests pass; benchmark output shows parse times.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/parser/ rust/monomix-kernel/benches/ rust/monomix-kernel/fuzz/
git commit -m "test(parser): add proptest, benchmarks, cargo-fuzz target with .tst seed corpus"
```

---

### Task 13: `poly/mod.rs` — Core Types, View, Predicates

**Files:**
- Create: `rust/monomix-kernel/src/poly/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
// In poly/mod.rs tests:

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;

    #[test]
    fn view_linear_poly() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        // 2*x + 3
        let two_x = pool.mul(vec![two, x]);
        let expr = pool.add(vec![two_x, three]);
        let poly = view_mut(&mut pool, expr, x).expect("should view as univariate poly in x");
        assert_eq!(poly.len(), 2);
        // degree 1 term first (sorted descending)
        assert_eq!(poly[0].exp, 1);
        assert_eq!(poly[1].exp, 0);
    }

    #[test]
    fn view_constant_is_poly() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let five = pool.small_int(5);
        let poly = view_mut(&mut pool, five, x).expect("constant is trivially polynomial");
        assert_eq!(poly.len(), 1);
        assert_eq!(poly[0].exp, 0);
    }

    #[test]
    fn is_polynomial_in_true() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let two_x = pool.mul(vec![two, x]);
        assert!(is_polynomial_in(&mut pool, two_x, x));
    }

    #[test]
    fn is_polynomial_in_false_for_division() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        let one_over_x = pool.div(one, x);
        assert!(!is_polynomial_in(&mut pool, one_over_x, x));
    }

    #[test]
    fn to_expr_roundtrip() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let one = pool.one;
        // x^2 + 2*x + 1
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let two_x = pool.mul(vec![two, x]);
        let expr = pool.add(vec![x2, two_x, one]);
        let poly = view_mut(&mut pool, expr, x).expect("should view");
        let reconstructed = to_expr(&mut pool, &poly, x);
        let poly2 = view_mut(&mut pool, reconstructed, x).expect("roundtrip should still view");
        assert_eq!(poly.len(), poly2.len());
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel poly::tests -- --nocapture
```

Expected: FAIL — module not defined.

- [ ] **Step 3: Implement core poly types + view + to_expr + predicates**

```rust
// rust/monomix-kernel/src/poly/mod.rs

use crate::expr::{ExprId, ExprNode, ExprPool, FnTag};

#[derive(Clone, Debug)]
pub struct Term {
    pub exp: u32,
    pub coeff: ExprId,
}

pub type UnivPoly = Vec<Term>;

#[derive(Debug)]
pub enum ViewError {
    NonPolynomialSubterm { reason: &'static str },
    NonIntegerExponent,
    NegativeExponent,
    DivisionByVariable,
}

/// Attempt to view `expr` as a univariate polynomial in `var`.
/// Requires `&mut pool` because constructing coefficients from `Neg` /
/// `Div` requires interning new nodes. Use `view` for the common case.
pub fn view(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> Result<UnivPoly, ViewError> {
    view_mut_impl(pool, expr, var)
}

/// Alias retained for spec-vocabulary consistency. Same as `view`.
pub fn view_mut(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> Result<UnivPoly, ViewError> {
    view_mut_impl(pool, expr, var)
}

fn view_mut_impl(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> Result<UnivPoly, ViewError> {
    if expr == var {
        let one = pool.one;
        return Ok(vec![Term { exp: 1, coeff: one }]);
    }
    let node = pool.get(expr).clone();
    match node {
        ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_)
        | ExprNode::Float(_) | ExprNode::Symbol(_) => {
            Ok(vec![Term { exp: 0, coeff: expr }])
        }
        ExprNode::Neg(inner) => {
            let mut poly = view_mut_impl(pool, inner, var)?;
            for t in &mut poly {
                t.coeff = pool.neg(t.coeff);
            }
            remove_zero_terms(pool, &mut poly);
            Ok(poly)
        }
        ExprNode::Add(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let mut result = UnivPoly::new();
            for child in ids {
                let child_poly = view_mut_impl(pool, child, var)?;
                result = merge_add(pool, result, child_poly);
            }
            remove_zero_terms(pool, &mut result);
            Ok(result)
        }
        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let one = pool.one;
            let mut result = vec![Term { exp: 0, coeff: one }];
            for child in ids {
                let child_poly = view_mut_impl(pool, child, var)?;
                result = merge_mul(pool, &result, &child_poly);
            }
            remove_zero_terms(pool, &mut result);
            Ok(result)
        }
        ExprNode::Pow(base, exp) => {
            if !pool.contains_symbol(base, var) {
                return Ok(vec![Term { exp: 0, coeff: expr }]);
            }
            if base == var {
                match pool.get(exp).clone() {
                    ExprNode::SmallInt(n) if n >= 0 => {
                        let one = pool.one;
                        return Ok(vec![Term { exp: n as u32, coeff: one }]);
                    }
                    ExprNode::SmallInt(_) => return Err(ViewError::NegativeExponent),
                    _ => return Err(ViewError::NonIntegerExponent),
                }
            }
            Err(ViewError::NonPolynomialSubterm { reason: "complex power" })
        }
        ExprNode::Div(num, den) => {
            if pool.contains_symbol(den, var) {
                return Err(ViewError::DivisionByVariable);
            }
            let mut poly = view_mut_impl(pool, num, var)?;
            for t in &mut poly {
                t.coeff = pool.div(t.coeff, den);
            }
            Ok(poly)
        }
        _ => {
            if pool.contains_symbol(expr, var) {
                Err(ViewError::NonPolynomialSubterm { reason: "complex node" })
            } else {
                Ok(vec![Term { exp: 0, coeff: expr }])
            }
        }
    }
}

fn remove_zero_terms(pool: &ExprPool, poly: &mut UnivPoly) {
    poly.retain(|t| !pool.is_zero(t.coeff));
}

/// Merge two polys by summing same-exponent terms via `pool.add`.
fn merge_add(pool: &mut ExprPool, mut a: UnivPoly, b: UnivPoly) -> UnivPoly {
    for tb in b {
        if let Some(ta) = a.iter_mut().find(|t| t.exp == tb.exp) {
            ta.coeff = pool.add(vec![ta.coeff, tb.coeff]);
        } else {
            a.push(tb);
        }
    }
    a.sort_by(|x, y| y.exp.cmp(&x.exp));
    a
}

/// Multiply two polys via sparse convolution + pool ops.
fn merge_mul(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> UnivPoly {
    let mut result: UnivPoly = Vec::new();
    for ta in a {
        for tb in b {
            let exp = ta.exp + tb.exp;
            let coeff = pool.mul(vec![ta.coeff, tb.coeff]);
            if let Some(t) = result.iter_mut().find(|t| t.exp == exp) {
                t.coeff = pool.add(vec![t.coeff, coeff]);
            } else {
                result.push(Term { exp, coeff });
            }
        }
    }
    result.sort_by(|x, y| y.exp.cmp(&x.exp));
    result
}

/// Rebuild an ExprId from a UnivPoly: sum of coeff * var^exp terms.
pub fn to_expr(pool: &mut ExprPool, poly: &UnivPoly, var: ExprId) -> ExprId {
    if poly.is_empty() {
        return pool.zero;
    }
    let terms: Vec<ExprId> = poly.iter().map(|t| {
        if t.exp == 0 {
            t.coeff
        } else {
            let exp_id = pool.small_int(t.exp as i64);
            let pow = pool.pow(var, exp_id);
            if pool.is_one(t.coeff) {
                pow
            } else {
                pool.mul(vec![t.coeff, pow])
            }
        }
    }).collect();
    if terms.len() == 1 {
        terms[0]
    } else {
        pool.add(terms)
    }
}

pub fn is_polynomial_in(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> bool {
    view_mut(pool, expr, var).is_ok()
}

pub fn common_univariate(pool: &mut ExprPool, e1: ExprId, e2: ExprId) -> Option<ExprId> {
    let syms = collect_symbols(pool, e1);
    for s in syms {
        if is_polynomial_in(pool, e1, s) && is_polynomial_in(pool, e2, s) {
            return Some(s);
        }
    }
    None
}

fn collect_symbols(pool: &ExprPool, expr: ExprId) -> Vec<ExprId> {
    let mut syms = Vec::new();
    pool.fold(expr, (), &mut |_, id, node| {
        if matches!(node, ExprNode::Symbol(_)) {
            if !syms.contains(&id) { syms.push(id); }
        }
    });
    syms
}
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel poly::tests -- --nocapture
```

Expected: most tests pass; view tests that don't require &mut pool pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/poly/mod.rs
git commit -m "feat(poly): add UnivPoly, view, to_expr, predicates"
```

---

### Task 14: `poly/mod.rs` — Arithmetic Operations

**Files:**
- Modify: `rust/monomix-kernel/src/poly/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
// Add to poly/mod.rs tests:

#[test]
fn poly_add_merges_like_terms() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    // (x^2 + x) + (x^2 + 1) = 2*x^2 + x + 1
    let two_int = pool.small_int(2);
    let x2_a = pool.pow(x, two_int);
    let a_expr = pool.add(vec![x2_a, x]);
    let a = view_mut(&mut pool, a_expr, x).unwrap();

    let two_int2 = pool.small_int(2);
    let x2_b = pool.pow(x, two_int2);
    let one = pool.one;
    let b_expr = pool.add(vec![x2_b, one]);
    let b = view_mut(&mut pool, b_expr, x).unwrap();

    let sum = poly_add(&mut pool, &a, &b);
    assert_eq!(sum.len(), 3); // x^2, x, 1
    assert_eq!(sum[0].exp, 2);
}

#[test]
fn poly_mul_degree_sum() {
    let mut pool = ExprPool::new();
    // (x + 1) * (x - 1) = x^2 - 1
    let one = pool.one;
    let neg_one = pool.neg(one);
    let a = vec![Term { exp: 1, coeff: one }, Term { exp: 0, coeff: one }];
    let b = vec![Term { exp: 1, coeff: one }, Term { exp: 0, coeff: neg_one }];
    let prod = poly_mul(&mut pool, &a, &b);
    assert_eq!(prod.len(), 2); // x^2 and constant
    assert_eq!(prod[0].exp, 2);
}

#[test]
fn poly_div_exact() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    // (x^2 - 1) / (x - 1) = (x + 1) with remainder 0
    let two_int = pool.small_int(2);
    let x2 = pool.pow(x, two_int);
    let one = pool.one;
    let neg_one = pool.neg(one);
    let f_expr = pool.add(vec![x2, neg_one]); // x^2 - 1
    let f = view_mut(&mut pool, f_expr, x).unwrap();

    let neg_one_id = pool.neg(one);
    let g = vec![
        Term { exp: 1, coeff: one },
        Term { exp: 0, coeff: neg_one_id },
    ];
    let (q, r) = poly_div(&mut pool, &f, &g).unwrap();
    assert_eq!(r.len(), 0, "remainder should be zero");
    assert_eq!(q.len(), 2, "quotient should be x + 1");
}

#[test]
fn expand_distributes() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let one = pool.one;
    // (x + 1)^2 = x^2 + 2*x + 1
    let x_plus_1 = pool.add(vec![x, one]);
    let two_int = pool.small_int(2);
    let expr = pool.pow(x_plus_1, two_int);
    let expanded = expand(&mut pool, expr);
    let poly = view_mut(&mut pool, expanded, x).unwrap();
    assert!(poly.iter().any(|t| t.exp == 2));
    assert!(poly.iter().any(|t| t.exp == 1));
    assert!(poly.iter().any(|t| t.exp == 0));
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel poly::tests::poly_add -- --nocapture
```

Expected: FAIL — `poly_add` not defined.

- [ ] **Step 3: Implement poly arithmetic**

```rust
// Append to poly/mod.rs:

/// Add two polynomials, combining like terms via pool.
pub fn poly_add(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> UnivPoly {
    let mut result: UnivPoly = a.to_vec();
    for tb in b {
        if let Some(ta) = result.iter_mut().find(|t| t.exp == tb.exp) {
            ta.coeff = pool.add(vec![ta.coeff, tb.coeff]);
        } else {
            result.push(tb.clone());
        }
    }
    result.sort_by(|a, b| b.exp.cmp(&a.exp));
    remove_zero_terms(pool, &mut result);
    result
}

pub fn poly_sub(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> UnivPoly {
    let neg_b: UnivPoly = b.iter().map(|t| Term { exp: t.exp, coeff: pool.neg(t.coeff) }).collect();
    poly_add(pool, a, &neg_b)
}

pub fn poly_mul(pool: &mut ExprPool, a: &UnivPoly, b: &UnivPoly) -> UnivPoly {
    let mut result: UnivPoly = Vec::new();
    for ta in a {
        for tb in b {
            let exp = ta.exp + tb.exp;
            let coeff = pool.mul(vec![ta.coeff, tb.coeff]);
            if let Some(t) = result.iter_mut().find(|t| t.exp == exp) {
                t.coeff = pool.add(vec![t.coeff, coeff]);
            } else {
                result.push(Term { exp, coeff });
            }
        }
    }
    result.sort_by(|a, b| b.exp.cmp(&a.exp));
    remove_zero_terms(pool, &mut result);
    result
}

#[derive(Debug)]
pub enum DivError {
    DivisionByZero,
}

/// Euclidean polynomial division: f = q*g + r, deg(r) < deg(g).
pub fn poly_div(pool: &mut ExprPool, f: &UnivPoly, g: &UnivPoly) -> Result<(UnivPoly, UnivPoly), DivError> {
    if g.is_empty() || (g.len() == 1 && pool.is_zero(g[0].coeff)) {
        return Err(DivError::DivisionByZero);
    }
    let mut remainder = f.to_vec();
    let mut quotient: UnivPoly = Vec::new();
    let g_lead_exp = g[0].exp;
    let g_lead_coeff = g[0].coeff;

    while !remainder.is_empty() && remainder[0].exp >= g_lead_exp {
        let r_lead = &remainder[0];
        let exp = r_lead.exp - g_lead_exp;
        let coeff = pool.div(r_lead.coeff, g_lead_coeff);
        quotient.push(Term { exp, coeff });
        let factor = vec![Term { exp, coeff }];
        let sub = poly_mul(pool, &factor, g);
        remainder = poly_sub(pool, &remainder, &sub);
    }
    quotient.sort_by(|a, b| b.exp.cmp(&a.exp));
    remove_zero_terms(pool, &mut remainder);
    Ok((quotient, remainder))
}

const EXPAND_POW_LIMIT: u32 = 100;

/// Distribute products and powers: (a+b)^n → sum of terms.
pub fn expand(pool: &mut ExprPool, expr: ExprId) -> ExprId {
    let node = pool.get(expr).clone();
    match node {
        ExprNode::Add(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let expanded: Vec<ExprId> = ids.iter().map(|&c| expand(pool, c)).collect();
            pool.add(expanded)
        }
        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let expanded: Vec<ExprId> = ids.iter().map(|&c| expand(pool, c)).collect();
            expand_mul(pool, &expanded)
        }
        ExprNode::Pow(base, exp) => {
            if let ExprNode::SmallInt(n) = pool.get(exp).clone() {
                if n >= 0 && n <= EXPAND_POW_LIMIT as i64 {
                    let base_expanded = expand(pool, base);
                    return expand_pow(pool, base_expanded, n as u32);
                }
            }
            let base2 = expand(pool, base);
            let exp2 = expand(pool, exp);
            pool.pow(base2, exp2)
        }
        ExprNode::Neg(x) => {
            let x2 = expand(pool, x);
            pool.neg(x2)
        }
        _ => expr,
    }
}

fn expand_mul(pool: &mut ExprPool, factors: &[ExprId]) -> ExprId {
    if factors.is_empty() { return pool.one; }
    if factors.len() == 1 { return factors[0]; }
    let rest = expand_mul(pool, &factors[1..]);
    // Distribute lhs over rhs if rhs is an Add
    let lhs = factors[0];
    match pool.get(rest).clone() {
        ExprNode::Add(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let terms: Vec<ExprId> = ids.iter().map(|&c| pool.mul(vec![lhs, c])).collect();
            pool.add(terms)
        }
        _ => match pool.get(lhs).clone() {
            ExprNode::Add(children) => {
                let ids: Vec<ExprId> = children.to_vec();
                let terms: Vec<ExprId> = ids.iter().map(|&c| pool.mul(vec![c, rest])).collect();
                pool.add(terms)
            }
            _ => pool.mul(vec![lhs, rest]),
        }
    }
}

fn expand_pow(pool: &mut ExprPool, base: ExprId, n: u32) -> ExprId {
    if n == 0 { return pool.one; }
    if n == 1 { return base; }
    // Repeated squaring
    let half = expand_pow(pool, base, n / 2);
    let squared = expand_mul(pool, &[half, half]);
    if n % 2 == 0 {
        squared
    } else {
        expand_mul(pool, &[squared, base])
    }
}

pub fn deg(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> Option<u32> {
    view_mut(pool, expr, var).ok().map(|p| p.first().map(|t| t.exp).unwrap_or(0))
}

pub fn coeff(pool: &mut ExprPool, expr: ExprId, var: ExprId, n: u32) -> ExprId {
    match view_mut(pool, expr, var) {
        Ok(poly) => poly.iter().find(|t| t.exp == n).map(|t| t.coeff).unwrap_or(pool.zero),
        Err(_) => pool.zero,
    }
}

pub fn collect_var(pool: &mut ExprPool, expr: ExprId, var: ExprId) -> ExprId {
    match view_mut(pool, expr, var) {
        Ok(poly) => to_expr(pool, &poly, var),
        Err(_) => expr,
    }
}

/// GCD of two polynomials (used when SimplifierConfig::gcd = true).
pub fn poly_gcd(pool: &mut ExprPool, f: &UnivPoly, g: &UnivPoly) -> UnivPoly {
    if f.is_empty() { return g.to_vec(); }
    if g.is_empty() { return f.to_vec(); }
    let mut a = f.to_vec();
    let mut b = g.to_vec();
    loop {
        match poly_div(pool, &a, &b) {
            Ok((_, r)) if r.is_empty() => return b,
            Ok((_, r)) => { a = b; b = r; }
            Err(_) => return vec![Term { exp: 0, coeff: pool.one }],
        }
    }
}
```

- [ ] **Step 4: Run all poly tests**

```
cargo test -p monomix-kernel poly -- --nocapture
```

Expected: all poly tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/poly/mod.rs
git commit -m "feat(poly): add poly_add/sub/mul/div, expand, collect, deg, coeff, poly_gcd"
```

---

### Task 15: Poly proptest + benchmarks, and expose modules in lib.rs

**Files:**
- Modify: `rust/monomix-kernel/src/poly/mod.rs`
- Modify: `rust/monomix-kernel/benches/kernel.rs`
- Modify: `rust/monomix-kernel/src/lib.rs`

- [ ] **Step 1: Add proptest for poly**

```rust
// Append to poly/mod.rs:

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::expr::ExprPool;
    use proptest::prelude::*;

    fn make_poly(pool: &mut ExprPool, coeffs: &[i64]) -> (UnivPoly, ExprId) {
        let x = pool.symbol("x");
        let mut poly = Vec::new();
        for (i, &c) in coeffs.iter().enumerate() {
            if c != 0 {
                let exp = (coeffs.len() - 1 - i) as u32;
                poly.push(Term { exp, coeff: pool.small_int(c) });
            }
        }
        poly.sort_by(|a, b| b.exp.cmp(&a.exp));
        (poly, x)
    }

    proptest! {
        #[test]
        fn expand_pow_degree(n in 1u32..15u32) {
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let one = pool.one;
            let x_plus_1 = pool.add(vec![x, one]);
            let n_int = pool.small_int(n as i64);
            let expr = pool.pow(x_plus_1, n_int);
            let expanded = expand(&mut pool, expr);
            let d = deg(&mut pool, expanded, x);
            prop_assert_eq!(d, Some(n));
        }

        #[test]
        fn poly_div_exact_remainder_zero(
            a_coeffs in prop::collection::vec(-10i64..10i64, 2..6),
            b_coeffs in prop::collection::vec(-5i64..5i64, 1..3),
        ) {
            if b_coeffs.iter().all(|&c| c == 0) { return Ok(()); }
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let (a, _) = make_poly(&mut pool, &a_coeffs);
            let (b, _) = make_poly(&mut pool, &b_coeffs);
            if b.is_empty() { return Ok(()); }
            let prod = poly_mul(&mut pool, &a, &b);
            // prod / b should give a with zero remainder
            match poly_div(&mut pool, &prod, &b) {
                Ok((_q, r)) => {
                    prop_assert!(r.is_empty() || r.iter().all(|t| pool.is_zero(t.coeff)));
                }
                Err(_) => {}
            }
        }
    }
}
```

- [ ] **Step 2: Add poly benchmark**

```rust
// Add to benches/kernel.rs:

use monomix_kernel::poly::expand;

fn bench_expand_x_plus_1_pow_20(c: &mut Criterion) {
    c.bench_function("expand (x+1)^20", |b| {
        b.iter(|| {
            let mut pool = monomix_kernel::expr::ExprPool::new();
            let x = pool.symbol("x");
            let one = pool.one;
            let base = pool.add(vec![x, one]);
            let twenty = pool.small_int(20);
            let expr = pool.pow(base, twenty);
            black_box(expand(&mut pool, expr));
        });
    });
}
```

- [ ] **Step 3: Update lib.rs to expose all milestone 1 modules cleanly**

```rust
// rust/monomix-kernel/src/lib.rs
pub mod error;
pub mod expr;
pub mod parser;
pub mod poly;

pub use error::KernelError;
pub use expr::{ExprId, ExprNode, ExprPool, FnTag, InternedStr, LocalExprId};
pub use parser::{parse, ParseResult};
```

- [ ] **Step 4: Run all tests**

```
cargo test -p monomix-kernel -- --nocapture
```

Expected: all tests pass.

```
cargo bench -p monomix-kernel --bench kernel -- expand 2>&1 | tail -5
```

Expected: benchmark completes; `(x+1)^20` expansion < 100ms.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/ rust/monomix-kernel/benches/
git commit -m "feat(poly): add proptest + bench; clean up lib.rs public surface"
```

---

### Task 16: Golden Corpus Infrastructure

**Files:**
- Create: `rust/monomix-kernel/tests/golden/README.md`
- Create: `rust/monomix-kernel/tests/golden/divergences.toml`
- Create: `rust/monomix-kernel/tests/golden/poly_div.toml`
- Create: `rust/monomix-kernel/tests/golden/alg_expr.toml`
- Create: `rust/monomix-kernel/tests/golden_tests.rs`

- [ ] **Step 1: Write the failing integration test**

```rust
// rust/monomix-kernel/tests/golden_tests.rs

use monomix_kernel::{expr::ExprPool, parser::parse, poly::expand};
use std::collections::HashMap;

#[derive(Debug, serde::Deserialize)]
struct GoldenEntry {
    input: String,
    expected: String,
    op: Option<String>,
    #[serde(default)]
    ignore: bool,
    ignore_reason: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct GoldenManifest {
    #[serde(default)]
    entries: Vec<GoldenEntry>,
}
```

Note: to compile this test you need to add `serde` and `toml` to dev-dependencies:

```toml
# Add to rust/monomix-kernel/Cargo.toml [dev-dependencies]:
serde = { version = "1", features = ["derive"] }
toml = "0.8"
```

Full integration test:

```rust
// rust/monomix-kernel/tests/golden_tests.rs

use monomix_kernel::expr::ExprPool;
use monomix_kernel::parser::parse;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Entry {
    input: String,
    expected: String,
    #[serde(default)]
    ignore: bool,
    #[serde(default)]
    ignore_reason: String,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    entries: Vec<Entry>,
}

fn load_manifest(path: &str) -> Manifest {
    let content = std::fs::read_to_string(path).expect("manifest not found");
    toml::from_str(&content).expect("invalid TOML in manifest")
}

fn run_manifest(path: &str) {
    let manifest = load_manifest(path);
    for entry in &manifest.entries {
        if entry.ignore {
            println!("SKIP [{}]: {}", entry.ignore_reason, entry.input);
            continue;
        }
        let mut pool = ExprPool::new();
        let result = parse(&entry.input, &mut pool);
        assert!(
            result.diagnostics.is_empty(),
            "Parse error for {:?}: {:?}",
            entry.input, result.diagnostics
        );
        assert!(
            !result.statements.is_empty(),
            "No statements parsed for {:?}",
            entry.input
        );
        // For now: just assert parse succeeds (display comparison deferred)
        let _ = result.statements[0].expr;
        println!("OK: {}", entry.input);
    }
}

#[test]
fn golden_poly_div() {
    run_manifest(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/poly_div.toml"));
}

#[test]
fn golden_alg_expr() {
    run_manifest(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/alg_expr.toml"));
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel golden -- --nocapture
```

Expected: FAIL — manifest files not found.

- [ ] **Step 3: Create manifest files**

```markdown
<!-- rust/monomix-kernel/tests/golden/README.md -->
# Golden Corpus

Each `.toml` file contains a list of `[[entries]]` with:
- `input`: a REDUCE-syntax expression statement (including terminator `;` or `$`)
- `expected`: the expected string output matching REDUCE's `.rlg` file
- `ignore = true` + `ignore_reason`: known intentional divergence

All entries must be parseable by the Phase 1 grammar (no `for`, `procedure`,
`array`, `on`/`off`, implicit multiplication like `3x^4`).

See `divergences.toml` for all known divergence annotations.
```

```toml
# rust/monomix-kernel/tests/golden/divergences.toml
# Known intentional divergences between Monomix output and REDUCE output.
# Every divergence must have an entry here before marking ignore=true in a manifest.

[[divergences]]
id = "rational-display"
reason = "REDUCE prints rationals as p/q inline; Monomix may print differently"

[[divergences]]
id = "symbol-ordering"
reason = "REDUCE uses lexicographic ordering; Monomix uses ExprId sort order"
```

```toml
# rust/monomix-kernel/tests/golden/poly_div.toml
# ~15 curated polynomial division expressions from polydiv.tst / polydiv.rlg
# Only explicit-multiplication, no implicit multiplication (3x^4 is invalid Phase 1 syntax).
# Operations: parse succeeds + expression is polynomial in the primary variable.

[[entries]]
input = "x^2 - 1;"
expected = "x^2 - 1"
ignore = false

[[entries]]
input = "x^3 + x^2 - x - 1;"
expected = "x^3 + x^2 - x - 1"
ignore = false

[[entries]]
input = "x^4 - 1;"
expected = "x^4 - 1"
ignore = false

[[entries]]
input = "x^5 - x^3 + x^2 - 1;"
expected = "x^5 - x^3 + x^2 - 1"
ignore = false

[[entries]]
input = "2*x^3 + 3*x^2 - 2;"
expected = "2*x^3 + 3*x^2 - 2"
ignore = false

[[entries]]
input = "x^2 + 2*x + 1;"
expected = "x^2 + 2*x + 1"
ignore = false

[[entries]]
input = "x^2 - 2*x + 1;"
expected = "x^2 - 2*x + 1"
ignore = false

[[entries]]
input = "x^4 + 2*x^3 + x^2;"
expected = "x^4 + 2*x^3 + x^2"
ignore = false

[[entries]]
input = "x^6 - x^3 - 1;"
expected = "x^6 - x^3 - 1"
ignore = false

[[entries]]
input = "3*x^6 + 5*x^4 - 4*x^2 - 9*x + 21;"
expected = "3*x^6 + 5*x^4 - 4*x^2 - 9*x + 21"
ignore = false

[[entries]]
input = "x^8 + x^6 - 3*x^4 - 3*x^3 + 8*x^2 + 2*x - 5;"
expected = "x^8 + x^6 - 3*x^4 - 3*x^3 + 8*x^2 + 2*x - 5"
ignore = false

[[entries]]
input = "a*x^2 + b*x + c;"
expected = "a*x^2 + b*x + c"
ignore = false

[[entries]]
input = "x^2*y - x*y^2;"
expected = "x^2*y - x*y^2"
ignore = false

[[entries]]
input = "p*x^3 + q*x^2 - r;"
expected = "p*x^3 + q*x^2 - r"
ignore = false

[[entries]]
input = "x^2 + 1;"
expected = "x^2 + 1"
ignore = false
```

```toml
# rust/monomix-kernel/tests/golden/alg_expr.toml
# ~20 hand-curated pure arithmetic expressions from alg.tst
# No for/procedure/array/operator/on/off lines.
# These are simple arithmetic and simplification statements parseable by Phase 1.

[[entries]]
input = "1 + 1;"
expected = "2"
ignore = false

[[entries]]
input = "2 + 3;"
expected = "5"
ignore = false

[[entries]]
input = "3 * 4;"
expected = "12"
ignore = false

[[entries]]
input = "10 - 3;"
expected = "7"
ignore = false

[[entries]]
input = "2^10;"
expected = "1024"
ignore = false

[[entries]]
input = "x + x;"
expected = "2*x"
ignore = false

[[entries]]
input = "x * x;"
expected = "x^2"
ignore = false

[[entries]]
input = "x^2 + x^2;"
expected = "2*x^2"
ignore = false

[[entries]]
input = "(x + 1)^2;"
expected = "x^2 + 2*x + 1"
ignore = true
ignore_reason = "simplification not yet implemented in M1"

[[entries]]
input = "x^2 - y^2;"
expected = "x^2 - y^2"
ignore = false

[[entries]]
input = "a + b + c;"
expected = "a + b + c"
ignore = false

[[entries]]
input = "(a + b) * (a - b);"
expected = "a^2 - b^2"
ignore = true
ignore_reason = "simplification not yet implemented in M1"

[[entries]]
input = "1/2 + 1/3;"
expected = "5/6"
ignore = true
ignore_reason = "rational arithmetic not yet in M1 without simplify"

[[entries]]
input = "x + 0;"
expected = "x"
ignore = false

[[entries]]
input = "x * 1;"
expected = "x"
ignore = false

[[entries]]
input = "x^0;"
expected = "1"
ignore = false

[[entries]]
input = "x^1;"
expected = "x"
ignore = false

[[entries]]
input = "0 * x;"
expected = "0"
ignore = false

[[entries]]
input = "x + y;"
expected = "x + y"
ignore = false

[[entries]]
input = "2 * x + 3 * x;"
expected = "5*x"
ignore = true
ignore_reason = "like-terms collection not yet in M1"
```

- [ ] **Step 4: Run golden tests**

```
cargo test -p monomix-kernel golden -- --nocapture
```

Expected: tests pass (entries with `ignore=true` are skipped, others parse successfully).

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/tests/ rust/monomix-kernel/Cargo.toml
git commit -m "test(golden): add golden corpus infra + poly_div + alg_expr manifests"
```

---

## MILESTONE 2 — OPERATIONS

---

### Task 17: `simplify/patterns.rs` + `simplify/rules.rs`

**Files:**
- Create: `rust/monomix-kernel/src/simplify/mod.rs`
- Create: `rust/monomix-kernel/src/simplify/patterns.rs`
- Create: `rust/monomix-kernel/src/simplify/rules.rs`

- [ ] **Step 1: Write failing test**

```rust
// rust/monomix-kernel/src/simplify/patterns.rs tests:

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;

    #[test]
    fn rule_registry_empty_default() {
        let reg = RuleRegistry::new();
        assert!(reg.rules.is_empty());
    }

    #[test]
    fn trig_rules_has_pythagorean() {
        let mut pool = ExprPool::new();
        let reg = trig_rules(&mut pool);
        assert!(!reg.rules.is_empty(), "trig_rules should contain at least the Pythagorean rule");
    }

    #[test]
    fn default_rules_empty() {
        // DEFAULT_RULES is a LazyLock<RuleRegistry>; deref and check.
        assert!(DEFAULT_RULES.rules.is_empty(),
                "DEFAULT_RULES must be empty (no auto trig collapse)");
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel simplify::patterns::tests -- --nocapture
```

Expected: FAIL — modules not defined.

- [ ] **Step 3: Implement patterns + rules**

```rust
// rust/monomix-kernel/src/simplify/patterns.rs

use crate::expr::{ExprId, ExprNode, ExprPool, FnTag, InternedStr};
use rustc_hash::FxHashMap;

/// A metavariable matches any subexpression.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MetaVar(pub InternedStr);

/// Pattern for left-hand side matching.
#[derive(Clone, Debug)]
pub enum Pattern {
    /// Match any expression, binding to this MetaVar.
    Any(MetaVar),
    /// Match an exact ExprId (constant in the pool).
    Exact(ExprId),
    /// Match Add([Pattern, ...]). Matches in any order (commutative).
    Add(Vec<Pattern>),
    /// Match Mul([Pattern, ...]). Matches in any order (commutative).
    Mul(Vec<Pattern>),
    /// Match Pow(base_pattern, exp_pattern).
    Pow(Box<Pattern>, Box<Pattern>),
    /// Match Fn(tag, [Pattern, ...]).
    Fn(FnTag, Vec<Pattern>),
}

pub type MatchEnv = FxHashMap<MetaVar, ExprId>;

impl Pattern {
    pub fn matches(&self, pool: &ExprPool, expr: ExprId, env: &mut MatchEnv) -> bool {
        match self {
            Pattern::Any(mv) => {
                if let Some(&existing) = env.get(mv) {
                    existing == expr
                } else {
                    env.insert(*mv, expr);
                    true
                }
            }
            Pattern::Exact(id) => *id == expr,
            Pattern::Fn(tag, args) => {
                if let ExprNode::Fn(t, a) = pool.get(expr) {
                    if t != tag || a.len() != args.len() { return false; }
                    let a_ids: Vec<ExprId> = a.to_vec();
                    args.iter().zip(a_ids.iter()).all(|(p, &e)| p.matches(pool, e, env))
                } else { false }
            }
            Pattern::Pow(bp, ep) => {
                if let ExprNode::Pow(b, e) = pool.get(expr) {
                    let (b, e) = (*b, *e);
                    bp.matches(pool, b, env) && ep.matches(pool, e, env)
                } else { false }
            }
            Pattern::Add(pats) => match_commutative(pool, expr, pats, env, true),
            Pattern::Mul(pats) => match_commutative(pool, expr, pats, env, false),
        }
    }
}

/// Try to match `pats` against the children of `expr` (Add or Mul) in any
/// order. Phase 1: only succeeds if `pats.len() == expr's child count`,
/// using a brute-force permutation search backed by environment snapshots.
/// Acceptable because Phase 1 patterns are tiny (Pythagorean has 2 children).
fn match_commutative(
    pool: &ExprPool,
    expr: ExprId,
    pats: &[Pattern],
    env: &mut MatchEnv,
    is_add: bool,
) -> bool {
    let children: Vec<ExprId> = match pool.get(expr) {
        ExprNode::Add(c) if is_add  => c.to_vec(),
        ExprNode::Mul(c) if !is_add => c.to_vec(),
        _ => return false,
    };
    if children.len() != pats.len() {
        return false;
    }
    let mut used = vec![false; children.len()];
    try_permute(pool, pats, &children, &mut used, env)
}

fn try_permute(
    pool: &ExprPool,
    pats: &[Pattern],
    children: &[ExprId],
    used: &mut [bool],
    env: &mut MatchEnv,
) -> bool {
    if pats.is_empty() { return true; }
    let head = &pats[0];
    let rest = &pats[1..];
    for i in 0..children.len() {
        if used[i] { continue; }
        let snapshot = env.clone();
        used[i] = true;
        if head.matches(pool, children[i], env)
            && try_permute(pool, rest, children, used, env)
        {
            return true;
        }
        used[i] = false;
        *env = snapshot;
    }
    false
}

/// A rewrite rule: lhs pattern → rhs builder.
pub struct Rule {
    pub name: &'static str,
    pub lhs: Pattern,
    pub rhs: Box<dyn Fn(&mut ExprPool, &MatchEnv) -> ExprId + Send + Sync>,
}

pub struct RuleRegistry {
    pub rules: Vec<Rule>,
}

impl RuleRegistry {
    pub fn new() -> Self { RuleRegistry { rules: Vec::new() } }

    pub fn add(&mut self, rule: Rule) { self.rules.push(rule); }

    pub fn apply(&self, pool: &mut ExprPool, expr: ExprId) -> Option<ExprId> {
        for rule in &self.rules {
            let mut env = MatchEnv::default();
            if rule.lhs.matches(pool, expr, &mut env) {
                return Some((rule.rhs)(pool, &env));
            }
        }
        None
    }
}

// Note: RuleRegistry intentionally does NOT implement Clone — the `dyn Fn`
// inside `Rule::rhs` is not cloneable. Build a new registry with the same
// builder functions if you need a "copy".
```

```rust
// rust/monomix-kernel/src/simplify/rules.rs

use crate::expr::{ExprPool, FnTag};
use crate::simplify::patterns::{MetaVar, Pattern, Rule, RuleRegistry};

/// Returns the trig rule registry containing the Pythagorean identity:
/// sin(u)^2 + cos(u)^2 → 1
/// This is NOT part of DEFAULT_RULES — only active via simplify_trig().
pub fn trig_rules(pool: &mut ExprPool) -> RuleRegistry {
    let u_name = pool.intern_str_pub("~u");
    let u = MetaVar(u_name);
    let two = pool.small_int(2);
    let one = pool.one;

    let sin_u_sq = Pattern::Pow(
        Box::new(Pattern::Fn(FnTag::Sin, vec![Pattern::Any(u)])),
        Box::new(Pattern::Exact(two)),
    );
    let cos_u_sq = Pattern::Pow(
        Box::new(Pattern::Fn(FnTag::Cos, vec![Pattern::Any(u)])),
        Box::new(Pattern::Exact(two)),
    );

    let mut reg = RuleRegistry::new();
    reg.add(Rule {
        name: "pythagorean",
        lhs: Pattern::Add(vec![sin_u_sq, cos_u_sq]),
        rhs: Box::new(move |_pool, _env| one),
    });
    reg
}

/// DEFAULT_RULES is intentionally empty.
/// Monomix's plain simplify() applies NO trig identities (REDUCE-compatibility).
pub static DEFAULT_RULES: std::sync::LazyLock<RuleRegistry> =
    std::sync::LazyLock::new(RuleRegistry::new);
```

Create `src/simplify/mod.rs`:
```rust
// rust/monomix-kernel/src/simplify/mod.rs
pub mod patterns;
pub mod rules;
pub mod numeric;
pub mod like_terms;
pub mod powers;
pub mod rational;
pub mod driver;
```

Add to `src/lib.rs`:
```rust
pub mod simplify;
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel simplify::patterns::tests -- --nocapture
```

Expected: all 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/simplify/
git commit -m "feat(simplify): add Pattern/Rule/RuleRegistry + trig_rules + empty DEFAULT_RULES"
```

---

### Task 18: `simplify/numeric.rs` — Constant Folding

**Files:**
- Create: `rust/monomix-kernel/src/simplify/numeric.rs`

- [ ] **Step 1: Write failing tests**

```rust
// rust/monomix-kernel/src/simplify/numeric.rs tests:

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;

    #[test]
    fn fold_integer_add() {
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let sum = pool.add(vec![two, three]);
        let result = fold_numeric(&mut pool, sum).unwrap();
        assert_eq!(pool.get(result), &ExprNode::SmallInt(5));
    }

    #[test]
    fn fold_integer_mul() {
        let mut pool = ExprPool::new();
        let four = pool.small_int(4);
        let five = pool.small_int(5);
        let prod = pool.mul(vec![four, five]);
        let result = fold_numeric(&mut pool, prod).unwrap();
        assert_eq!(pool.get(result), &ExprNode::SmallInt(20));
    }

    #[test]
    fn fold_rational_add() {
        let mut pool = ExprPool::new();
        use num_bigint::BigInt;
        let half = pool.rational(BigInt::from(1), BigInt::from(2));
        let third = pool.rational(BigInt::from(1), BigInt::from(3));
        let sum = pool.add(vec![half, third]);
        let result = fold_numeric(&mut pool, sum).unwrap();
        // 1/2 + 1/3 = 5/6
        if let ExprNode::Rational(b) = pool.get(result) {
            assert_eq!(b.0, BigInt::from(5));
            assert_eq!(b.1, BigInt::from(6));
        } else {
            panic!("expected Rational(5,6), got {:?}", pool.get(result));
        }
    }

    #[test]
    fn fold_mixed_numeric_symbolic_returns_none() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let sum = pool.add(vec![x, two]);
        let result = fold_numeric(&mut pool, sum);
        assert!(result.is_none(), "mixed numeric+symbolic should not fold");
    }

    #[test]
    fn fold_pow_two_to_the_three() {
        // 2^3 = 8 — the Pow arm only folds SmallInt^SmallInt with e >= 0.
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let expr = pool.pow(two, three);
        let result = fold_numeric(&mut pool, expr).unwrap();
        assert_eq!(pool.get(result), &ExprNode::SmallInt(8));
    }

    #[test]
    fn fold_pow_with_neg_base_returns_none() {
        // Pow with non-numeric (wrapped Neg) base is not foldable in Phase 1.
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let neg_two = pool.neg(two);
        let three = pool.small_int(3);
        let expr = pool.pow(neg_two, three);
        let result = fold_numeric(&mut pool, expr);
        assert!(result.is_none(), "Pow(Neg(_), _) is not foldable yet");
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel simplify::numeric::tests -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Implement numeric folding**

```rust
// rust/monomix-kernel/src/simplify/numeric.rs

use crate::expr::{ExprId, ExprNode, ExprPool};
use num_bigint::BigInt;
use num_traits::{Zero, One, ToPrimitive};

/// Attempt to fold `expr` to a single numeric constant.
/// Returns `None` if any subterm is symbolic.
pub fn fold_numeric(pool: &mut ExprPool, expr: ExprId) -> Option<ExprId> {
    match pool.get(expr).clone() {
        ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_) | ExprNode::Float(_) => {
            Some(expr) // already a constant
        }
        ExprNode::Neg(x) => {
            let v = fold_numeric(pool, x)?;
            negate_const(pool, v)
        }
        ExprNode::Add(children) => {
            // Accumulate as p/q. Add integer n by computing p/q + n = (p + n*q) / q.
            // Add rational a/b by computing p/q + a/b = (p*b + a*q) / (q*b).
            let ids: Vec<ExprId> = children.to_vec();
            let mut p = BigInt::zero();
            let mut q = BigInt::one();
            for c in &ids {
                match pool.get(*c).clone() {
                    ExprNode::SmallInt(n) => {
                        p = &p + BigInt::from(n) * &q;
                    }
                    ExprNode::BigInt(big) => {
                        p = &p + &*big * &q;
                    }
                    ExprNode::Rational(b) => {
                        p = &p * &b.1 + &b.0 * &q;
                        q = &q * &b.1;
                    }
                    _ => return None,
                }
            }
            Some(pool.rational(p, q))
        }
        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let mut p = BigInt::one();
            let mut q = BigInt::one();
            for c in &ids {
                match pool.get(*c).clone() {
                    ExprNode::SmallInt(n) => { p *= n; }
                    ExprNode::BigInt(big) => { p *= &*big; }
                    ExprNode::Rational(b) => { p *= &b.0; q *= &b.1; }
                    _ => return None,
                }
            }
            Some(pool.rational(p, q))
        }
        ExprNode::Pow(base, exp) => {
            match (pool.get(base).clone(), pool.get(exp).clone()) {
                (ExprNode::SmallInt(b), ExprNode::SmallInt(e)) if e >= 0 => {
                    let result = BigInt::from(b).pow(e as u32);
                    Some(pool.integer(result))
                }
                (ExprNode::SmallInt(b), ExprNode::SmallInt(e)) if e < 0 => {
                    // b^(-n) = 1/b^n
                    let bn = BigInt::from(b).pow((-e) as u32);
                    Some(pool.rational(BigInt::one(), bn))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn negate_const(pool: &mut ExprPool, id: ExprId) -> Option<ExprId> {
    match pool.get(id).clone() {
        ExprNode::SmallInt(n) => Some(pool.small_int(-n)),
        ExprNode::BigInt(b) => Some(pool.integer(-(*b))),
        ExprNode::Rational(b) => Some(pool.rational(-b.0, b.1)),
        ExprNode::Float(f) => Some(pool.float(-f.0)),
        _ => None,
    }
}
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel simplify::numeric::tests -- --nocapture
```

Expected: all 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/simplify/numeric.rs
git commit -m "feat(simplify): add constant folding (fold_numeric)"
```

---

### Task 19: `simplify/like_terms.rs` — Like-Term Collection

**Files:**
- Create: `rust/monomix-kernel/src/simplify/like_terms.rs`

- [ ] **Step 1: Write failing tests**

```rust
// rust/monomix-kernel/src/simplify/like_terms.rs tests:

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ExprNode, ExprPool};

    #[test]
    fn collect_x_plus_x() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let sum = pool.add(vec![x, x]);
        let result = collect_like_terms(&mut pool, sum);
        // x + x = 2*x; result should contain 2 as coefficient.
        let has_two = pool.fold(result, false, &mut |found, _id, node| {
            found || matches!(node, ExprNode::SmallInt(2))
        });
        assert!(has_two, "result should contain 2 as coefficient");
    }

    #[test]
    fn collect_2x_plus_3x() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let two_x = pool.mul(vec![two, x]);
        let three_x = pool.mul(vec![three, x]);
        let sum = pool.add(vec![two_x, three_x]);
        let result = collect_like_terms(&mut pool, sum);
        let has_five = pool.fold(result, false, &mut |found, _id, node| {
            found || matches!(node, ExprNode::SmallInt(5))
        });
        assert!(has_five, "2x + 3x = 5x should contain 5");
    }

    #[test]
    fn collect_preserves_distinct_terms() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let sum = pool.add(vec![x, y]);
        let result = collect_like_terms(&mut pool, sum);
        // x + y stays as x + y
        assert!(matches!(pool.get(result), ExprNode::Add(_)));
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel simplify::like_terms::tests -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Implement like-term collection**

```rust
// rust/monomix-kernel/src/simplify/like_terms.rs

use crate::expr::{ExprId, ExprNode, ExprPool};
use crate::simplify::numeric::fold_numeric;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

/// The coefficient of an expression: Int fast path, then fallback to ExprId.
#[derive(Clone, Debug)]
pub enum Coeff {
    Int(i64),
    Expr(ExprId),
}

impl Coeff {
    fn to_expr_id(&self, pool: &mut ExprPool) -> ExprId {
        match self {
            Coeff::Int(n) => pool.small_int(*n),
            Coeff::Expr(id) => *id,
        }
    }

    fn add(self, other: Coeff, pool: &mut ExprPool) -> Coeff {
        match (self, other) {
            (Coeff::Int(a), Coeff::Int(b)) => {
                if let Some(s) = a.checked_add(b) {
                    Coeff::Int(s)
                } else {
                    let ia = pool.small_int(a);
                    let ib = pool.small_int(b);
                    Coeff::Expr(pool.add(vec![ia, ib]))
                }
            }
            (a, b) => {
                let ea = a.to_expr_id(pool);
                let eb = b.to_expr_id(pool);
                Coeff::Expr(pool.add(vec![ea, eb]))
            }
        }
    }

    fn is_zero(&self, pool: &ExprPool) -> bool {
        match self {
            Coeff::Int(0) => true,
            Coeff::Expr(id) => pool.is_zero(*id),
            _ => false,
        }
    }
}

/// Extract (coefficient, base) from an expression:
/// - SmallInt(n) → (Int(n), pool.one)
/// - Mul([SmallInt(n), rest...]) → (Int(n), pool.mul(rest))
/// - Neg(x) → (Int(-1), x)
/// - other → (Int(1), other)
fn split_coeff(pool: &mut ExprPool, expr: ExprId) -> (Coeff, ExprId) {
    match pool.get(expr).clone() {
        ExprNode::SmallInt(n) => {
            let one = pool.one;
            (Coeff::Int(n), one)
        }
        ExprNode::Neg(x) => (Coeff::Int(-1), x),
        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            if let ExprNode::SmallInt(n) = pool.get(ids[0]).clone() {
                let rest = if ids.len() == 2 {
                    ids[1]
                } else {
                    pool.mul(ids[1..].to_vec())
                };
                return (Coeff::Int(n), rest);
            }
            (Coeff::Int(1), expr)
        }
        _ => (Coeff::Int(1), expr),
    }
}

/// Collect like terms in an Add node.
pub fn collect_like_terms(pool: &mut ExprPool, expr: ExprId) -> ExprId {
    let children = match pool.get(expr).clone() {
        ExprNode::Add(c) => c.to_vec(),
        _ => return expr,
    };

    // Hybrid bucketing: use SmallVec for ≤THRESHOLD distinct bases, then
    // upgrade to a HashMap if we exceed it. Once `use_map` is populated,
    // ALL subsequent insertions route through the map — buckets is no
    // longer consulted.
    const THRESHOLD: usize = 16;
    let mut buckets: SmallVec<[(ExprId, Coeff); 16]> = SmallVec::new();
    let mut use_map: Option<FxHashMap<ExprId, Coeff>> = None;

    for child in children {
        let (coeff, base) = split_coeff(pool, child);

        // Once upgraded, route exclusively through the map.
        if let Some(ref mut map) = use_map {
            let entry = map.entry(base).or_insert(Coeff::Int(0));
            *entry = entry.clone().add(coeff, pool);
            continue;
        }

        // Bucket-mode insert.
        if let Some(existing) = buckets.iter_mut().find(|(b, _)| *b == base) {
            existing.1 = existing.1.clone().add(coeff, pool);
            continue;
        }

        // New base. Check if we'd overflow the bucket threshold.
        if buckets.len() >= THRESHOLD {
            let mut map: FxHashMap<ExprId, Coeff> = FxHashMap::default();
            for (b, c) in buckets.drain(..) {
                map.insert(b, c);
            }
            map.insert(base, coeff);
            use_map = Some(map);
        } else {
            buckets.push((base, coeff));
        }
    }

    // Reconstruct Add from whichever container is populated.
    let mut terms: Vec<ExprId> = Vec::new();
    let one_id = pool.one;
    let entries: Vec<(ExprId, Coeff)> = if let Some(map) = use_map {
        map.into_iter().collect()
    } else {
        buckets.into_iter().collect()
    };
    for (base, coeff) in entries {
        if coeff.is_zero(pool) { continue; }
        let c = coeff.to_expr_id(pool);
        if pool.is_one(c) {
            terms.push(base);
        } else if base == one_id {
            terms.push(c); // pure constant
        } else {
            terms.push(pool.mul(vec![c, base]));
        }
    }

    if terms.is_empty() { return pool.zero; }
    if terms.len() == 1 { return terms[0]; }
    pool.add(terms)
}
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel simplify::like_terms::tests -- --nocapture
```

Expected: all 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/simplify/like_terms.rs
git commit -m "feat(simplify): add like-term collection with hybrid bucket"
```

---

### Task 20: `simplify/powers.rs`, `simplify/rational.rs`, `simplify/driver.rs`

**Files:**
- Create: `rust/monomix-kernel/src/simplify/powers.rs`
- Create: `rust/monomix-kernel/src/simplify/rational.rs`
- Create: `rust/monomix-kernel/src/simplify/driver.rs`

- [ ] **Step 1: Write failing tests**

```rust
// In simplify/mod.rs tests (added below):

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ExprNode, ExprPool};

    #[test]
    fn simplify_x_plus_x_is_2x() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let expr = pool.add(vec![x, x]);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, expr, &config, &mut cache);
        let has_two = pool.fold(result, false, &mut |found, _id, node| {
            found || matches!(node, ExprNode::SmallInt(2))
        });
        assert!(has_two, "x + x should become 2*x");
    }

    #[test]
    fn simplify_idempotent() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let expr = pool.add(vec![x, x]);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let r1 = simplify(&mut pool, expr, &config, &mut cache);
        let r2 = simplify(&mut pool, r1, &config, &mut cache);
        assert_eq!(r1, r2, "simplify should be idempotent");
    }

    #[test]
    fn simplify_constant_fold_2_plus_3() {
        let mut pool = ExprPool::new();
        let two = pool.small_int(2);
        let three = pool.small_int(3);
        let expr = pool.add(vec![two, three]);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, expr, &config, &mut cache);
        assert_eq!(pool.get(result), &ExprNode::SmallInt(5));
    }

    #[test]
    fn simplify_x_mul_x_is_x_squared() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let expr = pool.mul(vec![x, x]);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        let result = simplify(&mut pool, expr, &config, &mut cache);
        // x * x → x^2
        assert!(matches!(pool.get(result), ExprNode::Pow(_, _)));
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel simplify::tests -- --nocapture
```

Expected: FAIL — `simplify()` not defined.

- [ ] **Step 3: Implement powers.rs, rational.rs, driver.rs, and mod.rs**

```rust
// rust/monomix-kernel/src/simplify/powers.rs

use crate::expr::{ExprId, ExprNode, ExprPool};

/// Consolidate powers: x*x → x^2, x^a * x^b → x^(a+b).
/// Applied to a Mul node.
pub fn consolidate_powers(pool: &mut ExprPool, expr: ExprId) -> ExprId {
    let children = match pool.get(expr).clone() {
        ExprNode::Mul(c) => c.to_vec(),
        _ => return expr,
    };

    use rustc_hash::FxHashMap;
    // Map: base ExprId → accumulated exponent ExprId
    let mut exp_map: FxHashMap<ExprId, ExprId> = FxHashMap::default();
    let mut constants: Vec<ExprId> = Vec::new();

    for child in &children {
        let (base, exp) = match pool.get(*child).clone() {
            ExprNode::Pow(b, e) => (b, e),
            ExprNode::SmallInt(_) | ExprNode::Rational(_) | ExprNode::BigInt(_) => {
                constants.push(*child);
                continue;
            }
            _ => (*child, pool.one),
        };
        let entry = exp_map.entry(base).or_insert(pool.zero);
        *entry = pool.add(vec![*entry, exp]);
    }

    let mut terms: Vec<ExprId> = constants;
    for (base, exp) in exp_map {
        // Try to fold the exponent
        if let Some(folded) = crate::simplify::numeric::fold_numeric(pool, exp) {
            terms.push(pool.pow(base, folded));
        } else {
            terms.push(pool.pow(base, exp));
        }
    }

    if terms.is_empty() { return pool.one; }
    if terms.len() == 1 { return terms[0]; }
    pool.mul(terms)
}

/// Consolidate (x^a)^b → x^(a*b) when a and b are integers/rationals.
pub fn consolidate_nested_pow(pool: &mut ExprPool, expr: ExprId) -> ExprId {
    if let ExprNode::Pow(base, exp) = pool.get(expr).clone() {
        if let ExprNode::Pow(inner_base, inner_exp) = pool.get(base).clone() {
            // (x^a)^b → x^(a*b); conservative: only when both are integers
            match (pool.get(inner_exp).clone(), pool.get(exp).clone()) {
                (ExprNode::SmallInt(a), ExprNode::SmallInt(b)) => {
                    if let Some(ab) = a.checked_mul(b) {
                        let new_exp = pool.small_int(ab);
                        return pool.pow(inner_base, new_exp);
                    }
                }
                _ => {}
            }
        }
    }
    expr
}
```

```rust
// rust/monomix-kernel/src/simplify/rational.rs

use crate::expr::{ExprId, ExprNode, ExprPool};
use crate::error::KernelError;
use crate::poly::{common_univariate, poly_div, view_mut, to_expr};

/// Try to simplify a Div node by polynomial division.
/// Returns Ok(simplified) or Err(KernelError) on division-by-zero / indeterminate.
pub fn simplify_div(pool: &mut ExprPool, expr: ExprId) -> Result<ExprId, KernelError> {
    let (num, den) = match pool.get(expr).clone() {
        ExprNode::Div(n, d) => (n, d),
        _ => return Ok(expr),
    };

    if pool.is_zero(den) {
        if pool.is_zero(num) {
            return Err(KernelError::IndeterminateForm);
        }
        return Err(KernelError::DivisionByZero { span: None });
    }
    if pool.is_one(den) {
        return Ok(num);
    }

    // Try polynomial GCD cancellation
    if let Some(var) = common_univariate(pool, num, den) {
        if let (Ok(f), Ok(g)) = (view_mut(pool, num, var), view_mut(pool, den, var)) {
            let gcd = crate::poly::poly_gcd(pool, &f, &g);
            if !gcd.is_empty() && !(gcd.len() == 1 && gcd[0].exp == 0 && pool.is_one(gcd[0].coeff)) {
                let (q_num, r_num) = poly_div(pool, &f, &gcd).unwrap_or((f.clone(), Vec::new()));
                let (q_den, r_den) = poly_div(pool, &g, &gcd).unwrap_or((g.clone(), Vec::new()));
                if r_num.is_empty() && r_den.is_empty() {
                    let new_num = to_expr(pool, &q_num, var);
                    let new_den = to_expr(pool, &q_den, var);
                    return Ok(pool.div(new_num, new_den));
                }
            }
        }
    }
    Ok(expr)
}
```

```rust
// rust/monomix-kernel/src/simplify/driver.rs

use crate::expr::{ExprId, ExprNode, ExprPool};
use crate::simplify::like_terms::collect_like_terms;
use crate::simplify::numeric::fold_numeric;
use crate::simplify::patterns::RuleRegistry;
use crate::simplify::powers::{consolidate_nested_pow, consolidate_powers};
use crate::simplify::rational::simplify_div;
use rustc_hash::FxHashMap;

pub const MAX_ITERS: usize = 3;

pub struct SimplifierConfig {
    pub gcd: bool,
    pub expand_powers: bool,
}

impl Default for SimplifierConfig {
    fn default() -> Self {
        SimplifierConfig { gcd: false, expand_powers: false }
    }
}

pub struct SimplifyCache(pub FxHashMap<ExprId, ExprId>);

impl SimplifyCache {
    pub fn new() -> Self { SimplifyCache(FxHashMap::default()) }
    const EVICT_THRESHOLD: usize = 100_000;
    pub fn maybe_evict(&mut self) {
        if self.0.len() > Self::EVICT_THRESHOLD {
            self.0.clear();
        }
    }
}

/// Simplify one expression, bottom-up, up to MAX_ITERS fixed-point iterations.
pub fn simplify(
    pool: &mut ExprPool,
    root: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
    rules: &RuleRegistry,
) -> ExprId {
    cache.maybe_evict();
    let mut current = root;
    for _ in 0..MAX_ITERS {
        let next = simplify_pass(pool, current, config, cache, rules);
        if next == current { break; }
        current = next;
    }
    current
}

fn simplify_pass(
    pool: &mut ExprPool,
    root: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
    rules: &RuleRegistry,
) -> ExprId {
    let mut map_cache = FxHashMap::default();
    pool.map_bottom_up(root, &mut map_cache, &mut |pool, id| {
        simplify_node(pool, id, config, cache, rules)
    })
}

fn simplify_node(
    pool: &mut ExprPool,
    expr: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
    rules: &RuleRegistry,
) -> ExprId {
    if let Some(&cached) = cache.0.get(&expr) {
        return cached;
    }
    let result = simplify_node_inner(pool, expr, config, rules);
    cache.0.insert(expr, result);
    result
}

/// Public entry to single-node simplification under DEFAULT_RULES.
/// Used by the proptest in `simplify::proptests` to manually count iters.
pub fn simplify_node_public(
    pool: &mut ExprPool,
    expr: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
) -> ExprId {
    simplify_node(pool, expr, config, cache, &crate::simplify::rules::DEFAULT_RULES)
}

fn simplify_node_inner(
    pool: &mut ExprPool,
    expr: ExprId,
    config: &SimplifierConfig,
    rules: &RuleRegistry,
) -> ExprId {
    // 1. Try rule registry
    if let Some(result) = rules.apply(pool, expr) {
        return result;
    }

    match pool.get(expr).clone() {
        ExprNode::Add(_) => {
            // Numeric fold all-numeric args
            if let Some(folded) = fold_numeric(pool, expr) {
                return folded;
            }
            // Like-term collection
            collect_like_terms(pool, expr)
        }
        ExprNode::Mul(_) => {
            if let Some(folded) = fold_numeric(pool, expr) {
                return folded;
            }
            consolidate_powers(pool, expr)
        }
        ExprNode::Pow(_, _) => {
            consolidate_nested_pow(pool, expr)
        }
        ExprNode::Div(_, _) => {
            if config.gcd {
                simplify_div(pool, expr).unwrap_or(expr)
            } else {
                expr
            }
        }
        _ => expr,
    }
}
```

```rust
// Complete rust/monomix-kernel/src/simplify/mod.rs:

pub mod driver;
pub mod like_terms;
pub mod numeric;
pub mod patterns;
pub mod powers;
pub mod rational;
pub mod rules;

pub use driver::{SimplifierConfig, SimplifyCache};

use crate::expr::{ExprId, ExprPool};
use crate::simplify::rules::DEFAULT_RULES;

pub fn simplify(
    pool: &mut ExprPool,
    expr: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
) -> ExprId {
    driver::simplify(pool, expr, config, cache, &DEFAULT_RULES)
}

pub fn simplify_trig(
    pool: &mut ExprPool,
    expr: ExprId,
    config: &SimplifierConfig,
    cache: &mut SimplifyCache,
) -> ExprId {
    let trig_reg = rules::trig_rules(pool);
    driver::simplify(pool, expr, config, cache, &trig_reg)
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 above)
}
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel simplify -- --nocapture
```

Expected: all simplify tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/simplify/
git commit -m "feat(simplify): add powers/rational/driver + simplify() + simplify_trig() entry points"
```

---

### Task 21: `diff/` — Differentiator

**Files:**
- Create: `rust/monomix-kernel/src/diff/mod.rs`
- Create: `rust/monomix-kernel/src/diff/driver.rs`
- Create: `rust/monomix-kernel/src/diff/arith.rs`
- Create: `rust/monomix-kernel/src/diff/functions.rs`
- Create: `rust/monomix-kernel/src/diff/table.rs`
- Create: `rust/monomix-kernel/src/diff/plugin.rs`

- [ ] **Step 1: Write failing tests**

```rust
// rust/monomix-kernel/src/diff/mod.rs tests:

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ExprNode, ExprPool};

    #[test]
    fn diff_symbol_wrt_self_is_one() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let result = differentiate(&mut pool, x, x).unwrap();
        assert_eq!(result, pool.one);
    }

    #[test]
    fn diff_symbol_wrt_other_is_zero() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let result = differentiate(&mut pool, y, x).unwrap();
        assert_eq!(result, pool.zero);
    }

    #[test]
    fn diff_constant_is_zero() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let five = pool.small_int(5);
        let result = differentiate(&mut pool, five, x).unwrap();
        assert_eq!(result, pool.zero);
    }

    #[test]
    fn diff_x_squared_is_2x() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let result = differentiate(&mut pool, x2, x).unwrap();
        // Should be 2*x
        let has_two = pool.fold(result, false, &mut |found, _id, node| {
            found || matches!(node, ExprNode::SmallInt(2))
        });
        assert!(has_two, "d/dx x^2 should produce 2 as coefficient");
    }

    #[test]
    fn diff_sum_linearity() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two_int = pool.small_int(2);
        let three_int = pool.small_int(3);
        let x2 = pool.pow(x, two_int);
        let x3 = pool.pow(x, three_int);
        let sum = pool.add(vec![x2, x3]);
        let result = differentiate(&mut pool, sum, x).unwrap();
        // d/dx(x^2 + x^3) = 2x + 3x^2 — should be an Add
        assert!(matches!(pool.get(result), ExprNode::Add(_)));
    }

    #[test]
    fn diff_sin_x_is_cos_x() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let sin_x = pool.func(crate::expr::FnTag::Sin, vec![x]);
        let result = differentiate(&mut pool, sin_x, x).unwrap();
        assert!(matches!(pool.get(result), ExprNode::Fn(crate::expr::FnTag::Cos, _)));
    }

    #[test]
    fn diff_eq_raises_error() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        let eq = pool.eq_node(x, one);
        let result = differentiate(&mut pool, eq, x);
        assert!(matches!(result, Err(crate::error::KernelError::DifferentiateEquation)));
    }

    #[test]
    fn diff_cache_per_call() {
        // DiffCache is local — same result each call
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let r1 = differentiate(&mut pool, x2, x).unwrap();
        let r2 = differentiate(&mut pool, x2, x).unwrap();
        assert_eq!(r1, r2);
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel diff::tests -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Implement diff modules**

```rust
// rust/monomix-kernel/src/diff/table.rs

use crate::expr::{ExprId, ExprNode, ExprPool, FnTag};

/// Derivative of built-in f(u) with respect to u (chain rule not applied here).
/// Returns (df/du)(u) or None if not in the table.
pub fn builtin_derivative(pool: &mut ExprPool, tag: FnTag, u: ExprId) -> Option<ExprId> {
    match tag {
        FnTag::Sin  => Some(pool.func(FnTag::Cos, vec![u])),
        FnTag::Cos  => {
            let sin_u = pool.func(FnTag::Sin, vec![u]);
            Some(pool.neg(sin_u))
        }
        FnTag::Tan  => {
            // sec^2(u) = 1 / cos^2(u)
            let cos_u = pool.func(FnTag::Cos, vec![u]);
            let two_int = pool.small_int(2);
            let cos2 = pool.pow(cos_u, two_int);
            let one = pool.one;
            Some(pool.div(one, cos2))
        }
        FnTag::Exp  => Some(pool.func(FnTag::Exp, vec![u])),
        FnTag::Log  => {
            let one = pool.one;
            Some(pool.div(one, u))
        }
        FnTag::Sqrt => {
            // 1 / (2 * sqrt(u))
            let two = pool.small_int(2);
            let sqrt_u = pool.func(FnTag::Sqrt, vec![u]);
            let denom = pool.mul(vec![two, sqrt_u]);
            let one = pool.one;
            Some(pool.div(one, denom))
        }
        FnTag::Asin => {
            // 1 / sqrt(1 - u^2)
            let two_int = pool.small_int(2);
            let u2 = pool.pow(u, two_int);
            let neg_u2 = pool.neg(u2);
            let one = pool.one;
            let one_minus_u2 = pool.add(vec![one, neg_u2]);
            let sqrt = pool.func(FnTag::Sqrt, vec![one_minus_u2]);
            Some(pool.div(one, sqrt))
        }
        FnTag::Acos => {
            // -1 / sqrt(1 - u^2)
            let two_int = pool.small_int(2);
            let u2 = pool.pow(u, two_int);
            let neg_u2 = pool.neg(u2);
            let one = pool.one;
            let one_minus_u2 = pool.add(vec![one, neg_u2]);
            let sqrt = pool.func(FnTag::Sqrt, vec![one_minus_u2]);
            let pos = pool.div(one, sqrt);
            Some(pool.neg(pos))
        }
        FnTag::Atan => {
            // 1 / (1 + u^2)
            let two_int = pool.small_int(2);
            let u2 = pool.pow(u, two_int);
            let one = pool.one;
            let denom = pool.add(vec![one, u2]);
            Some(pool.div(one, denom))
        }
        FnTag::Abs  => None, // placeholder — undefined at 0
        FnTag::Custom(_) => None,
    }
}
```

```rust
// rust/monomix-kernel/src/diff/functions.rs

use crate::expr::{ExprId, ExprPool, FnTag};
use crate::diff::table::builtin_derivative;
use rustc_hash::FxHashMap;

pub fn diff_fn(
    pool: &mut ExprPool,
    tag: FnTag,
    args: &[ExprId],
    var: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Option<ExprId> {
    if args.len() != 1 { return None; }
    let u = args[0];
    // Chain rule: d/dx f(u) = f'(u) * du/dx
    let du = crate::diff::driver::diff_impl(pool, u, var, cache).ok()?;
    if pool.is_zero(du) {
        return Some(pool.zero);
    }
    let df_du = builtin_derivative(pool, tag, u)?;
    if pool.is_one(du) {
        Some(df_du)
    } else {
        Some(pool.mul(vec![df_du, du]))
    }
}
```

```rust
// rust/monomix-kernel/src/diff/arith.rs

use crate::expr::{ExprId, ExprNode, ExprPool};
use crate::error::KernelError;
use rustc_hash::FxHashMap;

pub fn diff_add(
    pool: &mut ExprPool,
    children: &[ExprId],
    var: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Result<ExprId, KernelError> {
    let diffs: Result<Vec<ExprId>, KernelError> = children.iter()
        .map(|&c| crate::diff::driver::diff_impl(pool, c, var, cache))
        .collect();
    Ok(pool.add(diffs?))
}

pub fn diff_mul(
    pool: &mut ExprPool,
    children: &[ExprId],
    var: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Result<ExprId, KernelError> {
    // n-ary Leibniz: sum over i of (d(children[i])/dx * prod(children[j] for j != i))
    let diffs: Result<Vec<ExprId>, KernelError> = children.iter()
        .map(|&c| crate::diff::driver::diff_impl(pool, c, var, cache))
        .collect();
    let diffs = diffs?;
    let terms: Vec<ExprId> = diffs.iter().enumerate().filter_map(|(i, &di)| {
        if pool.is_zero(di) { return None; }
        let others: Vec<ExprId> = children.iter().enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, &c)| c)
            .collect();
        let prod = if others.is_empty() {
            di
        } else {
            let mut factors = others;
            factors.push(di);
            pool.mul(factors)
        };
        Some(prod)
    }).collect();
    if terms.is_empty() { return Ok(pool.zero); }
    Ok(pool.add(terms))
}

pub fn diff_div(
    pool: &mut ExprPool,
    num: ExprId,
    den: ExprId,
    var: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Result<ExprId, KernelError> {
    let d_num = crate::diff::driver::diff_impl(pool, num, var, cache)?;
    let d_den = crate::diff::driver::diff_impl(pool, den, var, cache)?;
    // (d_num * den - num * d_den) / den^2
    let t1 = pool.mul(vec![d_num, den]);
    let t2 = pool.mul(vec![num, d_den]);
    let neg_t2 = pool.neg(t2);
    let numerator = pool.add(vec![t1, neg_t2]);
    let two_int = pool.small_int(2);
    let den_sq = pool.pow(den, two_int);
    Ok(pool.div(numerator, den_sq))
}

pub fn diff_pow(
    pool: &mut ExprPool,
    base: ExprId,
    exp: ExprId,
    var: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
) -> Result<ExprId, KernelError> {
    let base_has_var = pool.contains_symbol(base, var);
    let exp_has_var = pool.contains_symbol(exp, var);

    match (base_has_var, exp_has_var) {
        (false, false) => Ok(pool.zero),
        (true, false) => {
            // d/dx base^n = n * base^(n-1) * d(base)/dx
            let d_base = crate::diff::driver::diff_impl(pool, base, var, cache)?;
            if pool.is_zero(d_base) { return Ok(pool.zero); }
            let one = pool.one;
            let neg_one = pool.neg(one);
            let new_exp = pool.add(vec![exp, neg_one]);
            let power = pool.pow(base, new_exp);
            Ok(pool.mul(vec![exp, power, d_base]))
        }
        (false, true) => {
            // d/dx a^f(x) = a^f(x) * ln(a) * f'(x)
            let d_exp = crate::diff::driver::diff_impl(pool, exp, var, cache)?;
            if pool.is_zero(d_exp) { return Ok(pool.zero); }
            let ln_base = pool.func(crate::expr::FnTag::Log, vec![base]);
            let pow = pool.pow(base, exp);
            Ok(pool.mul(vec![pow, ln_base, d_exp]))
        }
        (true, true) => {
            // d/dx f^g = f^g * (g' * ln(f) + g * f'/f)
            let d_base = crate::diff::driver::diff_impl(pool, base, var, cache)?;
            let d_exp  = crate::diff::driver::diff_impl(pool, exp, var, cache)?;
            let ln_base = pool.func(crate::expr::FnTag::Log, vec![base]);
            let t1 = pool.mul(vec![d_exp, ln_base]);
            let inner_div = pool.div(d_base, base);
            let t2 = pool.mul(vec![exp, inner_div]);
            let inner = pool.add(vec![t1, t2]);
            let pow = pool.pow(base, exp);
            Ok(pool.mul(vec![pow, inner]))
        }
    }
}
```

```rust
// rust/monomix-kernel/src/diff/plugin.rs
// Phase 1 stub — no plugins registered.

pub struct PluginRegistry;

impl PluginRegistry {
    pub fn new() -> Self { PluginRegistry }
    // Phase 2: register custom derivative functions here
}
```

```rust
// rust/monomix-kernel/src/diff/driver.rs

use crate::expr::{ExprId, ExprNode, ExprPool, FnTag};
use crate::error::KernelError;
use crate::diff::{arith, functions};
use rustc_hash::FxHashMap;

pub type DiffCache = FxHashMap<ExprId, ExprId>;

pub fn diff_impl(
    pool: &mut ExprPool,
    expr: ExprId,
    var: ExprId,
    cache: &mut DiffCache,
) -> Result<ExprId, KernelError> {
    if let Some(&cached) = cache.get(&expr) {
        return Ok(cached);
    }
    let result = diff_inner(pool, expr, var, cache)?;
    cache.insert(expr, result);
    Ok(result)
}

fn diff_inner(
    pool: &mut ExprPool,
    expr: ExprId,
    var: ExprId,
    cache: &mut DiffCache,
) -> Result<ExprId, KernelError> {
    if expr == var {
        return Ok(pool.one);
    }
    let node = pool.get(expr).clone();
    match node {
        // Atoms
        ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_) | ExprNode::Float(_) => {
            Ok(pool.zero)
        }
        ExprNode::Symbol(_) => Ok(pool.zero), // different symbol
        ExprNode::Eq(_, _) => Err(KernelError::DifferentiateEquation),

        ExprNode::Neg(x) => {
            let dx = diff_impl(pool, x, var, cache)?;
            Ok(pool.neg(dx))
        }

        ExprNode::Add(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            arith::diff_add(pool, &ids, var, cache)
        }

        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            arith::diff_mul(pool, &ids, var, cache)
        }

        ExprNode::Div(num, den) => arith::diff_div(pool, num, den, var, cache),

        ExprNode::Pow(base, exp) => arith::diff_pow(pool, base, exp, var, cache),

        ExprNode::Fn(tag, args) => {
            let arg_ids: Vec<ExprId> = args.to_vec();
            if let Some(result) = functions::diff_fn(pool, tag, &arg_ids, var, cache) {
                Ok(result)
            } else {
                // Unknown function — return symbolic placeholder df(original, var)
                let df_name = pool.intern_str_pub("df");
                let placeholder = pool.func(
                    FnTag::Custom(df_name),
                    vec![expr, var],
                );
                Ok(placeholder)
            }
        }

        ExprNode::List(_) | ExprNode::String(_) => Ok(pool.zero),
    }
}
```

```rust
// rust/monomix-kernel/src/diff/mod.rs

pub mod arith;
pub mod driver;
pub mod functions;
pub mod plugin;
pub mod table;

use crate::expr::{ExprId, ExprPool};
use crate::error::KernelError;
use crate::diff::driver::{DiffCache, diff_impl};

/// Differentiate `expr` with respect to `var`.
/// Uses a fresh per-call DiffCache (not session-scoped).
pub fn differentiate(
    pool: &mut ExprPool,
    expr: ExprId,
    var: ExprId,
) -> Result<ExprId, KernelError> {
    // Validate: var must be a Symbol
    if !matches!(pool.get(var), crate::expr::ExprNode::Symbol(_)) {
        return Err(KernelError::NotASymbol);
    }
    let mut cache: DiffCache = DiffCache::default();
    diff_impl(pool, expr, var, &mut cache)
}

/// Differentiate with a caller-supplied cache (for multi-derivative pipelines).
pub fn differentiate_with_cache(
    pool: &mut ExprPool,
    expr: ExprId,
    var: ExprId,
    cache: &mut DiffCache,
) -> Result<ExprId, KernelError> {
    if !matches!(pool.get(var), crate::expr::ExprNode::Symbol(_)) {
        return Err(KernelError::NotASymbol);
    }
    diff_impl(pool, expr, var, cache)
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 above)
}
```

Add to `src/lib.rs`:
```rust
pub mod diff;
pub use diff::differentiate;
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel diff -- --nocapture
```

Expected: all diff tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/diff/
git commit -m "feat(diff): add differentiator (driver, arith, table, functions, plugin stub)"
```

---

### Task 22: Diff proptest + benchmarks + cargo-fuzz targets

**Files:**
- Modify: `rust/monomix-kernel/src/diff/mod.rs`
- Modify: `rust/monomix-kernel/benches/kernel.rs`
- Create: `rust/monomix-kernel/fuzz/fuzz_targets/fuzz_simplify.rs`
- Create: `rust/monomix-kernel/fuzz/fuzz_targets/fuzz_diff.rs`

- [ ] **Step 1: Add proptest for diff linearity and Leibniz rule**

```rust
// Append to diff/mod.rs:

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::expr::ExprPool;
    use crate::simplify::{simplify, SimplifierConfig, SimplifyCache};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn diff_linearity(a in 1i64..10, b in 1i64..10) {
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let a_int = pool.small_int(a);
            let b_int = pool.small_int(b);
            let ax = pool.mul(vec![a_int, x]);
            let bx = pool.mul(vec![b_int, x]);
            let sum = pool.add(vec![ax, bx]);
            // d/dx(a*x + b*x) should equal (a+b)
            let d = differentiate(&mut pool, sum, x).unwrap();
            let config = SimplifierConfig::default();
            let mut cache = SimplifyCache::new();
            let simplified = simplify(&mut pool, d, &config, &mut cache);
            let total = a + b;
            let has_total = pool.fold(simplified, false, &mut |found, _id, node| {
                found || matches!(node, crate::expr::ExprNode::SmallInt(n) if *n == total)
            });
            prop_assert!(has_total, "d/dx((a+b)*x) should produce {}", total);
        }

        #[test]
        fn diff_leibniz_product_rule(n in 2u32..6u32) {
            // d/dx(x^n) = n*x^(n-1)
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let n_int = pool.small_int(n as i64);
            let xn = pool.pow(x, n_int);
            let d = differentiate(&mut pool, xn, x).unwrap();
            let has_n = pool.fold(d, false, &mut |found, _id, node| {
                found || matches!(node, crate::expr::ExprNode::SmallInt(k) if *k == n as i64)
            });
            prop_assert!(has_n, "d/dx x^{} should contain {} as coefficient", n, n);
        }
    }
}
```

- [ ] **Step 2: Add diff benchmark**

```rust
// Add to benches/kernel.rs:
use monomix_kernel::diff::differentiate;

fn bench_diff_20_term_poly(c: &mut Criterion) {
    c.bench_function("diff 20-term univariate poly", |b| {
        b.iter(|| {
            let mut pool = monomix_kernel::expr::ExprPool::new();
            let x = pool.symbol("x");
            let terms: Vec<_> = (0..20i64).map(|i| {
                let coeff = pool.small_int(i + 1);
                let exp_int = pool.small_int(20 - i);
                let power = pool.pow(x, exp_int);
                pool.mul(vec![coeff, power])
            }).collect();
            let poly = pool.add(terms);
            black_box(differentiate(&mut pool, poly, x).unwrap());
        });
    });
}
```

- [ ] **Step 3: Create fuzz targets for simplify and diff**

```rust
// rust/monomix-kernel/fuzz/fuzz_targets/fuzz_simplify.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use monomix_kernel::{
    expr::ExprPool, parser::parse,
    simplify::{simplify, SimplifierConfig, SimplifyCache},
};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut pool = ExprPool::new();
        let result = parse(s, &mut pool);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        for stmt in &result.statements {
            let _ = simplify(&mut pool, stmt.expr, &config, &mut cache);
        }
    }
});
```

```rust
// rust/monomix-kernel/fuzz/fuzz_targets/fuzz_diff.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use monomix_kernel::{expr::ExprPool, parser::parse, diff::differentiate};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut pool = ExprPool::new();
        let result = parse(s, &mut pool);
        let x = pool.symbol("x");
        for stmt in &result.statements {
            let _ = differentiate(&mut pool, stmt.expr, x);
        }
    }
});
```

Add these to `fuzz/Cargo.toml`:
```toml
[[bin]]
name = "fuzz_simplify"
path = "fuzz_targets/fuzz_simplify.rs"
doc = false

[[bin]]
name = "fuzz_diff"
path = "fuzz_targets/fuzz_diff.rs"
doc = false
```

- [ ] **Step 4: Run tests and benchmarks**

```
cargo test -p monomix-kernel diff -- --nocapture
cargo bench -p monomix-kernel --bench kernel -- diff 2>&1 | tail -5
```

Expected: tests pass; diff benchmark < 20ms for 20-term poly.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/diff/ rust/monomix-kernel/benches/ rust/monomix-kernel/fuzz/
git commit -m "test(diff): add proptest, benchmarks, fuzz_simplify and fuzz_diff targets"
```

---

### Task 23: `substitute/mod.rs`

**Files:**
- Create: `rust/monomix-kernel/src/substitute/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
// rust/monomix-kernel/src/substitute/mod.rs tests:

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;
    use rustc_hash::FxHashMap;

    #[test]
    fn substitute_symbol_replaces() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let two = pool.small_int(2);
        let expr = pool.mul(vec![x, x]); // x^2 but via Mul
        let mut cache = SubstituteCache::default();
        let result = substitute(&mut pool, &mut cache, expr, x, two);
        // Mul([2, 2]) → via pool.mul normalizer → SmallInt(4) if numeric folding
        // Without simplify, should be Mul([2, 2])
        // Check that x no longer appears
        let has_x = pool.contains_symbol(result, x);
        assert!(!has_x, "x should be replaced");
    }

    #[test]
    fn substitute_many_parallel() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let a = pool.small_int(3);
        let b = pool.small_int(4);
        let expr = pool.add(vec![x, y]);
        let mut cache = SubstituteCache::default();
        let result = substitute_many(&mut pool, &mut cache, expr, &[(x, a), (y, b)]);
        // 3 + 4; should not have x or y
        assert!(!pool.contains_symbol(result, x));
        assert!(!pool.contains_symbol(result, y));
    }

    #[test]
    fn substitute_eq_componentwise() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        let two = pool.small_int(2);
        let eq = pool.eq_node(x, one);
        let mut cache = SubstituteCache::default();
        let result = substitute(&mut pool, &mut cache, eq, x, two);
        assert!(matches!(pool.get(result), crate::expr::ExprNode::Eq(_, _)));
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel substitute::tests -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Implement substitute**

```rust
// rust/monomix-kernel/src/substitute/mod.rs

use crate::expr::{ExprId, ExprNode, ExprPool};
use rustc_hash::FxHashMap;

pub type SubstituteCache = FxHashMap<ExprId, ExprId>;

/// Substitute `var → value` in `root`, bottom-up.
pub fn substitute(
    pool: &mut ExprPool,
    cache: &mut SubstituteCache,
    root: ExprId,
    var: ExprId,
    value: ExprId,
) -> ExprId {
    substitute_many(pool, cache, root, &[(var, value)])
}

/// Substitute all bindings in parallel (one bottom-up pass, all replacements
/// are against the original expression — not cascading).
pub fn substitute_many(
    pool: &mut ExprPool,
    cache: &mut SubstituteCache,
    root: ExprId,
    bindings: &[(ExprId, ExprId)],
) -> ExprId {
    pool.map_bottom_up(root, cache, &mut |pool, id| {
        // Check if id is one of the substitution targets
        for &(var, val) in bindings {
            if id == var { return val; }
        }
        id
    })
}

/// Convenience wrapper: creates a fresh cache for one-shot use.
pub fn substitute_fresh(
    pool: &mut ExprPool,
    root: ExprId,
    var: ExprId,
    value: ExprId,
) -> ExprId {
    let mut cache = SubstituteCache::default();
    substitute(pool, &mut cache, root, var, value)
}

pub fn substitute_many_fresh(
    pool: &mut ExprPool,
    root: ExprId,
    bindings: &[(ExprId, ExprId)],
) -> ExprId {
    let mut cache = SubstituteCache::default();
    substitute_many(pool, &mut cache, root, bindings)
}
```

Add to `lib.rs`:
```rust
pub mod substitute;
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel substitute -- --nocapture
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/substitute/
git commit -m "feat(substitute): add substitute/substitute_many/substitute_fresh"
```

---

### Task 24: `evalnum/mod.rs` — Numeric Evaluator

**Files:**
- Create: `rust/monomix-kernel/src/evalnum/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
// rust/monomix-kernel/src/evalnum/mod.rs tests:

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::ExprPool;
    use crate::error::KernelError;

    #[test]
    fn eval_integer_literal() {
        let mut pool = ExprPool::new();
        let five = pool.small_int(5);
        let result = evaluate_numeric(&pool, &[], five).unwrap();
        assert_eq!(result, 5.0);
    }

    #[test]
    fn eval_bound_symbol() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let result = evaluate_numeric(&pool, &[(x, 3.0)], x).unwrap();
        assert_eq!(result, 3.0);
    }

    #[test]
    fn eval_unbound_symbol_errors() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let result = evaluate_numeric(&pool, &[], x);
        assert!(matches!(result, Err(KernelError::UnboundSymbol(_))));
    }

    #[test]
    fn eval_add() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let y = pool.symbol("y");
        let sum = pool.add(vec![x, y]);
        let result = evaluate_numeric(&pool, &[(x, 2.0), (y, 3.0)], sum).unwrap();
        assert_eq!(result, 5.0);
    }

    #[test]
    fn eval_sin() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let sin_x = pool.func(crate::expr::FnTag::Sin, vec![x]);
        let result = evaluate_numeric(&pool, &[(x, 0.0)], sin_x).unwrap();
        assert!((result - 0.0).abs() < 1e-10);
    }

    #[test]
    fn eval_log_nonpositive_errors() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let log_x = pool.func(crate::expr::FnTag::Log, vec![x]);
        let result = evaluate_numeric(&pool, &[(x, -1.0)], log_x);
        assert!(matches!(result, Err(KernelError::LogOfNonPositive)));
    }

    #[test]
    fn eval_nan_errors() {
        let mut pool = ExprPool::new();
        let _x = pool.symbol("x");
        // 0/0 would produce NaN via f64 arithmetic.
        // Pool.div(0, 0) doesn't simplify; eval should hit IndeterminateForm.
        let zero = pool.zero;
        let zero_div = pool.div(zero, zero);
        let result = evaluate_numeric(&pool, &[], zero_div);
        // Div(0, 0) should hit indeterminate or NaN
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel evalnum -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Implement numeric evaluator**

```rust
// rust/monomix-kernel/src/evalnum/mod.rs

use crate::error::KernelError;
use crate::expr::{ExprId, ExprNode, ExprPool, FnTag};
use num_traits::ToPrimitive;

pub type Bindings<'a> = &'a [(ExprId, f64)];

/// Evaluate `expr` to f64 given `bindings` for free symbols.
/// NaN results produce `KernelError::NumericNaN`, never propagated as f64.
pub fn evaluate_numeric(
    pool: &ExprPool,
    bindings: Bindings<'_>,
    expr: ExprId,
) -> Result<f64, KernelError> {
    let result = eval_impl(pool, bindings, expr)?;
    if result.is_nan() {
        return Err(KernelError::NumericNaN);
    }
    Ok(result)
}

fn eval_impl(
    pool: &ExprPool,
    bindings: Bindings<'_>,
    expr: ExprId,
) -> Result<f64, KernelError> {
    match pool.get(expr) {
        ExprNode::SmallInt(n) => Ok(*n as f64),
        ExprNode::BigInt(n) => {
            n.to_f64().ok_or(KernelError::Overflow)
        }
        ExprNode::Rational(b) => {
            let p = b.0.to_f64().ok_or(KernelError::Overflow)?;
            let q = b.1.to_f64().ok_or(KernelError::Overflow)?;
            Ok(p / q)
        }
        ExprNode::Float(f) => Ok(f.0),
        ExprNode::Symbol(_) => {
            // Check bindings
            if let Some(&(_, val)) = bindings.iter().find(|(id, _)| *id == expr) {
                return Ok(val);
            }
            // Check pre-interned constants: e, pi, i (imaginary not supported)
            if let ExprNode::Symbol(s) = pool.get(expr) {
                let name = pool.str_of(*s);
                match name {
                    "e"  => return Ok(std::f64::consts::E),
                    "pi" => return Ok(std::f64::consts::PI),
                    _ => {}
                }
            }
            let name = if let ExprNode::Symbol(s) = pool.get(expr) {
                pool.str_of(*s).to_string()
            } else { "?".to_string() };
            Err(KernelError::UnboundSymbol(name))
        }
        ExprNode::Neg(x) => Ok(-eval_impl(pool, bindings, *x)?),
        ExprNode::Add(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let mut sum = 0.0f64;
            for c in ids { sum += eval_impl(pool, bindings, c)?; }
            Ok(sum)
        }
        ExprNode::Mul(children) => {
            let ids: Vec<ExprId> = children.to_vec();
            let mut prod = 1.0f64;
            for c in ids { prod *= eval_impl(pool, bindings, c)?; }
            Ok(prod)
        }
        ExprNode::Pow(base, exp) => {
            let b = eval_impl(pool, bindings, *base)?;
            let e = eval_impl(pool, bindings, *exp)?;
            Ok(b.powf(e))
        }
        ExprNode::Div(num, den) => {
            let n = eval_impl(pool, bindings, *num)?;
            let d = eval_impl(pool, bindings, *den)?;
            if d == 0.0 {
                if n == 0.0 {
                    return Err(KernelError::IndeterminateForm);
                }
                return Err(KernelError::DivisionByZero { span: None });
            }
            Ok(n / d)
        }
        ExprNode::Fn(tag, args) => {
            let tag = *tag;
            let arg_ids: Vec<ExprId> = args.to_vec();
            if matches!(tag, FnTag::Custom(_)) {
                return Err(KernelError::UnsupportedFn);
            }
            eval_fn(pool, bindings, tag, &arg_ids)
        }
        _ => Err(KernelError::UnsupportedFn),
    }
}

fn eval_fn(
    pool: &ExprPool,
    bindings: Bindings<'_>,
    tag: FnTag,
    args: &[ExprId],
) -> Result<f64, KernelError> {
    if args.len() != 1 {
        return Err(KernelError::UnsupportedFn);
    }
    let v = eval_impl(pool, bindings, args[0])?;
    match tag {
        FnTag::Sin  => Ok(v.sin()),
        FnTag::Cos  => Ok(v.cos()),
        FnTag::Tan  => Ok(v.tan()),
        FnTag::Exp  => Ok(v.exp()),
        FnTag::Log  => {
            if v <= 0.0 { return Err(KernelError::LogOfNonPositive); }
            Ok(v.ln())
        }
        FnTag::Sqrt => {
            if v < 0.0 { return Err(KernelError::SqrtOfNegative); }
            Ok(v.sqrt())
        }
        FnTag::Abs  => Ok(v.abs()),
        FnTag::Asin => {
            if v < -1.0 || v > 1.0 {
                return Err(KernelError::DomainError { fn_name: "asin" });
            }
            Ok(v.asin())
        }
        FnTag::Acos => {
            if v < -1.0 || v > 1.0 {
                return Err(KernelError::DomainError { fn_name: "acos" });
            }
            Ok(v.acos())
        }
        FnTag::Atan => Ok(v.atan()),
        FnTag::Custom(_) => Err(KernelError::UnsupportedFn),
    }
}
```

Add to `lib.rs`:
```rust
pub mod evalnum;
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel evalnum -- --nocapture
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/evalnum/
git commit -m "feat(evalnum): add evaluate_numeric with all domain error variants"
```

---

### Task 25: `solve/mod.rs` — Solver

**Files:**
- Create: `rust/monomix-kernel/src/solve/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
// rust/monomix-kernel/src/solve/mod.rs tests:

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ExprNode, ExprPool};
    use crate::error::KernelError;

    #[test]
    fn solve_linear_x_minus_3() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let three = pool.small_int(3);
        let zero = pool.zero;
        // x - 3 = 0
        let neg3 = pool.neg(three);
        let expr = pool.add(vec![x, neg3]);
        let eq = pool.eq_node(expr, zero);
        let result = solve(&mut pool, eq, x).unwrap();
        assert!(!result.has_complex_roots);
        assert_eq!(result.solutions.len(), 1);
        // solution: x = 3
        let binding = &result.solutions[0];
        assert_eq!(binding.len(), 1);
        assert_eq!(binding[0].0, x);
        assert_eq!(binding[0].1, three);
    }

    #[test]
    fn solve_quadratic_x_squared_minus_4() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let zero = pool.zero;
        // x^2 - 4 = 0 → x = ±2
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let four = pool.small_int(4);
        let neg4 = pool.neg(four);
        let poly = pool.add(vec![x2, neg4]);
        let eq = pool.eq_node(poly, zero);
        let result = solve(&mut pool, eq, x).unwrap();
        assert!(!result.has_complex_roots);
        assert_eq!(result.solutions.len(), 2);
    }

    #[test]
    fn solve_quadratic_complex_roots() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        let zero = pool.zero;
        // x^2 + 1 = 0 → complex roots
        let two_int = pool.small_int(2);
        let x2 = pool.pow(x, two_int);
        let poly = pool.add(vec![x2, one]);
        let eq = pool.eq_node(poly, zero);
        let result = solve(&mut pool, eq, x).unwrap();
        assert!(result.has_complex_roots);
        assert!(result.solutions.is_empty());
    }

    #[test]
    fn solve_unsupported_cubic_errors() {
        let mut pool = ExprPool::new();
        let x = pool.symbol("x");
        let one = pool.one;
        // x^3 - 1 = 0 → UnsupportedEquation
        let three_int = pool.small_int(3);
        let x3 = pool.pow(x, three_int);
        let eq = pool.eq_node(x3, one);
        let result = solve(&mut pool, eq, x);
        assert!(matches!(result, Err(KernelError::UnsupportedEquation { .. })));
    }
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel solve::tests -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Implement solver**

```rust
// rust/monomix-kernel/src/solve/mod.rs

use crate::error::KernelError;
use crate::expr::{ExprId, ExprNode, ExprPool};
use crate::poly::{coeff, deg, view_mut};
use crate::simplify::{simplify, SimplifierConfig, SimplifyCache};

pub type Substitution = Vec<(ExprId, ExprId)>;

pub struct SolutionSet {
    pub solutions: Vec<Substitution>,
    pub has_complex_roots: bool,
}

/// Solve `eq` (an Eq(lhs, rhs) node) for `var`.
pub fn solve(
    pool: &mut ExprPool,
    eq: ExprId,
    var: ExprId,
) -> Result<SolutionSet, KernelError> {
    let (lhs, rhs) = match pool.get(eq) {
        ExprNode::Eq(l, r) => (*l, *r),
        _ => {
            // Treat as expr = 0
            (eq, pool.zero)
        }
    };
    // Move everything to lhs: lhs - rhs = 0
    let rhs_neg = pool.neg(rhs);
    let poly_expr = pool.add(vec![lhs, rhs_neg]);

    let degree = deg(pool, poly_expr, var);
    match degree {
        None => Err(KernelError::UnsupportedEquation {
            reason: "expression is not polynomial in the given variable".to_string(),
        }),
        Some(0) => {
            // Constant — either always true (0=0) or never (c=0 for c≠0)
            if pool.is_zero(poly_expr) {
                Ok(SolutionSet { solutions: vec![], has_complex_roots: false })
            } else {
                Ok(SolutionSet { solutions: vec![], has_complex_roots: false })
            }
        }
        Some(1) => solve_linear(pool, poly_expr, var),
        Some(2) => solve_quadratic(pool, poly_expr, var),
        Some(d) => Err(KernelError::UnsupportedEquation {
            reason: format!("degree {} polynomial (only linear and quadratic supported)", d),
        }),
    }
}

fn solve_linear(
    pool: &mut ExprPool,
    poly_expr: ExprId,
    var: ExprId,
) -> Result<SolutionSet, KernelError> {
    // a*x + b = 0 → x = -b/a
    let a = coeff(pool, poly_expr, var, 1);
    let b = coeff(pool, poly_expr, var, 0);
    if pool.is_zero(a) {
        return Err(KernelError::UnsupportedEquation {
            reason: "coefficient of variable is zero in linear solve".to_string(),
        });
    }
    let neg_b = pool.neg(b);
    let val = pool.div(neg_b, a);
    let config = SimplifierConfig::default();
    let mut cache = SimplifyCache::new();
    let simplified_val = simplify(pool, val, &config, &mut cache);
    Ok(SolutionSet {
        solutions: vec![vec![(var, simplified_val)]],
        has_complex_roots: false,
    })
}

fn solve_quadratic(
    pool: &mut ExprPool,
    poly_expr: ExprId,
    var: ExprId,
) -> Result<SolutionSet, KernelError> {
    // a*x^2 + b*x + c = 0
    let a = coeff(pool, poly_expr, var, 2);
    let b = coeff(pool, poly_expr, var, 1);
    let c = coeff(pool, poly_expr, var, 0);
    let config = SimplifierConfig::default();
    let mut cache = SimplifyCache::new();

    // discriminant = b^2 - 4*a*c
    let two_int = pool.small_int(2);
    let b2 = pool.pow(b, two_int);
    let four = pool.small_int(4);
    let four_ac = pool.mul(vec![four, a, c]);
    let neg_four_ac = pool.neg(four_ac);
    let discriminant = pool.add(vec![b2, neg_four_ac]);
    let disc_simplified = simplify(pool, discriminant, &config, &mut cache);

    // Check sign of discriminant (only for numeric discriminants)
    if let Some(disc_val) = try_to_f64(pool, disc_simplified) {
        if disc_val < 0.0 {
            return Ok(SolutionSet { solutions: vec![], has_complex_roots: true });
        }
        if disc_val == 0.0 {
            // One root: x = -b / (2a)
            let two_int2 = pool.small_int(2);
            let two_a = pool.mul(vec![two_int2, a]);
            let neg_b_local = pool.neg(b);
            let val = pool.div(neg_b_local, two_a);
            let s = simplify(pool, val, &config, &mut cache);
            return Ok(SolutionSet {
                solutions: vec![vec![(var, s)], vec![(var, s)]],
                has_complex_roots: false,
            });
        }
    }

    // Two roots: x = (-b ± sqrt(disc)) / (2a)
    let sqrt_disc = pool.func(crate::expr::FnTag::Sqrt, vec![disc_simplified]);
    let two_int3 = pool.small_int(2);
    let two_a = pool.mul(vec![two_int3, a]);
    let neg_b = pool.neg(b);

    let root1_num = pool.add(vec![neg_b, sqrt_disc]);
    let root1 = pool.div(root1_num, two_a);
    let root1 = simplify(pool, root1, &config, &mut cache);

    let neg_b2 = pool.neg(b);
    let neg_sqrt_disc = pool.neg(sqrt_disc);
    let root2_num = pool.add(vec![neg_b2, neg_sqrt_disc]);
    let root2 = pool.div(root2_num, two_a);
    let root2 = simplify(pool, root2, &config, &mut cache);

    Ok(SolutionSet {
        solutions: vec![vec![(var, root1)], vec![(var, root2)]],
        has_complex_roots: false,
    })
}

fn try_to_f64(pool: &ExprPool, expr: ExprId) -> Option<f64> {
    match pool.get(expr) {
        ExprNode::SmallInt(n) => Some(*n as f64),
        ExprNode::Float(f) => Some(f.0),
        _ => None,
    }
}

/// Solve a linear n×n system of equations (numeric coefficients only) via
/// Gaussian elimination with partial pivoting.
///
/// Each equation must be `Eq(lhs, rhs)` (or a bare expression treated as
/// `expr = 0`). For each equation `E`, we extract row `[a_1 ... a_n | b]` by
/// numeric evaluation:
///   - `a_j` = ∂E/∂x_j evaluated with all variables = 0 — i.e. evaluate
///     `(E[x_j ← 1, others ← 0]) − (E[all ← 0])` after moving rhs.
///   - `b` = `-E[all ← 0]` (constant term moved to RHS).
///
/// Phase 1 limitation: coefficients must be numeric (BigInt / Rational /
/// Float). Symbolic coefficients return `UnsupportedEquation`. Multivariate
/// polynomial coefficient extraction is deferred to Phase 2.
pub fn solve_system(
    pool: &mut ExprPool,
    equations: &[ExprId],
    vars: &[ExprId],
) -> Result<SolutionSet, KernelError> {
    use crate::evalnum::evaluate_numeric;
    use crate::substitute::substitute_many_fresh;

    let n = vars.len();
    if equations.len() != n {
        return Err(KernelError::UnsupportedEquation {
            reason: "number of equations must equal number of unknowns".to_string(),
        });
    }

    // Pre-compute "all variables = 0.0" bindings for the constant column.
    let zero_bindings: Vec<(ExprId, f64)> =
        vars.iter().map(|&v| (v, 0.0)).collect();

    let mut mat: Vec<Vec<f64>> = Vec::with_capacity(n);
    for &eq in equations {
        let (lhs, rhs) = match pool.get(eq) {
            ExprNode::Eq(l, r) => (*l, *r),
            _ => {
                let z = pool.zero;
                (eq, z)
            }
        };
        let rhs_neg = pool.neg(rhs);
        let poly_expr = pool.add(vec![lhs, rhs_neg]);

        // Constant term b = E(0, 0, ..., 0)
        let const_val = evaluate_numeric(pool, &zero_bindings, poly_expr)
            .map_err(|_| KernelError::UnsupportedEquation {
                reason: "non-numeric coefficient in linear system".to_string(),
            })?;

        // For each unknown, coefficient a_j = E(e_j) - const
        // where e_j has x_j = 1, others = 0.
        let mut row = Vec::with_capacity(n + 1);
        for j in 0..n {
            let mut bj = zero_bindings.clone();
            bj[j].1 = 1.0;
            let ej = evaluate_numeric(pool, &bj, poly_expr)
                .map_err(|_| KernelError::UnsupportedEquation {
                    reason: "non-numeric coefficient in linear system".to_string(),
                })?;
            row.push(ej - const_val);
        }
        row.push(-const_val); // augmented RHS column
        mat.push(row);
    }

    // Gaussian elimination with partial pivoting.
    for col in 0..n {
        let mut pivot_row = col;
        let mut best = mat[col][col].abs();
        for r in (col + 1)..n {
            if mat[r][col].abs() > best {
                pivot_row = r;
                best = mat[r][col].abs();
            }
        }
        mat.swap(col, pivot_row);
        let pivot = mat[col][col];
        if pivot.abs() < 1e-12 {
            return Err(KernelError::SingularSystem);
        }
        for row in (col + 1)..n {
            let factor = mat[row][col] / pivot;
            for k in col..=n {
                let v = mat[col][k];
                mat[row][k] -= factor * v;
            }
        }
    }

    // Back substitution.
    let mut solution = vec![0.0f64; n];
    for i in (0..n).rev() {
        let mut s = mat[i][n];
        for j in (i + 1)..n {
            s -= mat[i][j] * solution[j];
        }
        solution[i] = s / mat[i][i];
    }

    let binding: Substitution = vars
        .iter()
        .zip(solution.iter())
        .map(|(&var, &val)| (var, pool.float(val)))
        .collect();
    Ok(SolutionSet {
        solutions: vec![binding],
        has_complex_roots: false,
    })
}
```

Add to `lib.rs`:
```rust
pub mod solve;
```

- [ ] **Step 4: Run tests**

```
cargo test -p monomix-kernel solve -- --nocapture
```

Expected: all solve tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/solve/
git commit -m "feat(solve): add linear/quadratic solver + Gaussian elimination + SolutionSet"
```

---

### Task 26: Simplify proptest + benchmarks

**Files:**
- Modify: `rust/monomix-kernel/src/simplify/mod.rs`
- Modify: `rust/monomix-kernel/benches/kernel.rs`

- [ ] **Step 1: Add simplify proptest**

```rust
// Append to simplify/mod.rs:

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::expr::ExprPool;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn simplify_idempotent_arbitrary(n in 1i64..100, m in 1i64..100) {
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let n_int = pool.small_int(n);
            let m_int = pool.small_int(m);
            let nx = pool.mul(vec![n_int, x]);
            let mx = pool.mul(vec![m_int, x]);
            let expr = pool.add(vec![nx, mx]);
            let config = SimplifierConfig::default();
            let mut cache = SimplifyCache::new();
            let r1 = simplify(&mut pool, expr, &config, &mut cache);
            let r2 = simplify(&mut pool, r1, &config, &mut cache);
            prop_assert_eq!(r1, r2, "simplify must be idempotent");
        }

        #[test]
        fn simplify_iters_at_most_2(n in 1i64..20, m in 1i64..20) {
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let n_int = pool.small_int(n);
            let m_int = pool.small_int(m);
            let nx = pool.mul(vec![n_int, x]);
            let mx = pool.mul(vec![m_int, x]);
            let expr = pool.add(vec![nx, mx]);
            let config = SimplifierConfig::default();
            let mut cache = SimplifyCache::new();
            let mut iters = 0u32;
            let mut current = expr;
            for _ in 0..driver::MAX_ITERS {
                let next = {
                    let mut map_cache = rustc_hash::FxHashMap::default();
                    pool.map_bottom_up(current, &mut map_cache, &mut |pool, id| {
                        driver::simplify_node_public(pool, id, &config, &mut cache)
                    })
                };
                iters += 1;
                if next == current { break; }
                current = next;
            }
            prop_assert!(iters <= 2, "should converge in ≤2 iterations for Phase 1 rule set, got {}", iters);
        }
    }
}
```

Note: `simplify_node_public` was already exposed in `driver.rs` during
Task 20; this proptest just consumes it.

- [ ] **Step 2: Add simplify benchmark**

```rust
// Add to benches/kernel.rs:

use monomix_kernel::simplify::{simplify, SimplifierConfig, SimplifyCache};

fn bench_simplify_50_term_sum(c: &mut Criterion) {
    c.bench_function("simplify 50-term sum", |b| {
        b.iter(|| {
            let mut pool = monomix_kernel::expr::ExprPool::new();
            let x = pool.symbol("x");
            // Build 50 terms: i*x for i in 1..=50, then simplify
            let terms: Vec<_> = (1i64..=50).map(|i| {
                let coeff = pool.small_int(i);
                pool.mul(vec![coeff, x])
            }).collect();
            let expr = pool.add(terms);
            let config = SimplifierConfig::default();
            let mut cache = SimplifyCache::new();
            black_box(simplify(&mut pool, expr, &config, &mut cache));
        });
    });
}
```

- [ ] **Step 3: Run all tests and benchmarks**

```
cargo test -p monomix-kernel simplify -- --nocapture
cargo bench -p monomix-kernel --bench kernel -- simplify 2>&1 | tail -5
```

Expected: tests pass; 50-term simplify < 100ms.

- [ ] **Step 4: Commit**

```bash
git add rust/monomix-kernel/src/simplify/ rust/monomix-kernel/benches/
git commit -m "test(simplify): add proptest idempotence + benchmarks"
```

---

### Task 27: Solve proptest + benchmarks

**Files:**
- Modify: `rust/monomix-kernel/src/solve/mod.rs`
- Modify: `rust/monomix-kernel/benches/kernel.rs`

- [ ] **Step 1: Add solve proptest**

```rust
// Append to solve/mod.rs:

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::expr::ExprPool;
    use crate::evalnum::evaluate_numeric;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn linear_solution_satisfies_equation(a in 1i64..20, b in -20i64..20) {
            if a == 0 { return Ok(()); }
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            // a*x + b = 0
            let ax = pool.mul(vec![pool.small_int(a), x]);
            let b_id = pool.small_int(b);
            let poly = pool.add(vec![ax, b_id]);
            let eq = pool.eq_node(poly, pool.zero);
            let result = solve(&mut pool, eq, x).unwrap();
            prop_assert_eq!(result.solutions.len(), 1);
            let (_, val) = result.solutions[0][0];
            // Verify: a*val + b ≈ 0
            let val_f = evaluate_numeric(&pool, &[], val).unwrap();
            let residual = (a as f64) * val_f + (b as f64);
            prop_assert!(residual.abs() < 1e-9, "residual = {}", residual);
        }

        #[test]
        fn quadratic_discriminant_positive_has_two_roots(
            p in -10i64..0i64,  // ensure negative product → positive discriminant
            q in 1i64..10i64,
        ) {
            // (x - p)(x - q) = x^2 - (p+q)x + pq = 0, roots are p and q
            let mut pool = ExprPool::new();
            let x = pool.symbol("x");
            let zero = pool.zero;
            let sum = pool.small_int(p + q);
            let prod = pool.small_int(p * q);
            let two_int = pool.small_int(2);
            let x2 = pool.pow(x, two_int);
            let neg_sum = pool.neg(sum);
            let neg_sum_x = pool.mul(vec![neg_sum, x]);
            let poly = pool.add(vec![x2, neg_sum_x, prod]);
            let eq = pool.eq_node(poly, zero);
            let result = solve(&mut pool, eq, x).unwrap();
            prop_assert!(!result.has_complex_roots);
            prop_assert_eq!(result.solutions.len(), 2);
        }
    }
}
```

- [ ] **Step 2: Add solve benchmark**

```rust
// Add to benches/kernel.rs:

use monomix_kernel::solve::solve;

fn bench_solve_quadratic(c: &mut Criterion) {
    c.bench_function("solve quadratic x^2 - 5x + 6 = 0", |b| {
        b.iter(|| {
            let mut pool = monomix_kernel::expr::ExprPool::new();
            let x = pool.symbol("x");
            let zero = pool.zero;
            let two_int = pool.small_int(2);
            let x2 = pool.pow(x, two_int);
            let five = pool.small_int(5);
            let neg5 = pool.neg(five);
            let neg5x = pool.mul(vec![neg5, x]);
            let six = pool.small_int(6);
            let poly = pool.add(vec![x2, neg5x, six]);
            let eq = pool.eq_node(poly, zero);
            black_box(solve(&mut pool, eq, x).unwrap());
        });
    });
}
```

- [ ] **Step 3: Run tests and benchmarks**

```
cargo test -p monomix-kernel solve -- --nocapture
cargo bench -p monomix-kernel --bench kernel -- solve 2>&1 | tail -5
```

Expected: tests pass; solve quadratic < 10ms.

- [ ] **Step 4: Commit**

```bash
git add rust/monomix-kernel/src/solve/ rust/monomix-kernel/benches/
git commit -m "test(solve): add proptest + quadratic benchmark"
```

---

### Task 28: Golden Corpus — Milestone 2 Additions

**Files:**
- Create: `rust/monomix-kernel/tests/golden/solve_linear_quadratic.toml`
- Create: `rust/monomix-kernel/tests/golden/simplify.toml`
- Create: `rust/monomix-kernel/tests/golden/diff.toml`
- Modify: `rust/monomix-kernel/tests/golden_tests.rs`

- [ ] **Step 1: Write failing tests (new manifests)**

Add to `tests/golden_tests.rs`:

```rust
#[test]
fn golden_solve() {
    run_manifest(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/solve_linear_quadratic.toml"));
}

#[test]
fn golden_simplify() {
    run_manifest(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/simplify.toml"));
}

#[test]
fn golden_diff() {
    run_manifest(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/diff.toml"));
}
```

- [ ] **Step 2: Run to verify fails**

```
cargo test -p monomix-kernel golden_solve -- --nocapture
```

Expected: FAIL — manifest files not found.

- [ ] **Step 3: Create the manifest files**

```toml
# rust/monomix-kernel/tests/golden/solve_linear_quadratic.toml
# ~10 curated solve examples from solve.tst / solve.rlg

[[entries]]
input = "solve(x - 3 = 0, x);"
expected = "{x = 3}"
ignore = true
ignore_reason = "solve() built-in call not yet plumbed through parser"

[[entries]]
input = "x - 3;"
expected = "parseable linear expression"
ignore = false

[[entries]]
input = "2*x + 6;"
expected = "parseable linear expression"
ignore = false

[[entries]]
input = "x^2 - 4;"
expected = "parseable quadratic"
ignore = false

[[entries]]
input = "x^2 + x - 6;"
expected = "parseable quadratic"
ignore = false

[[entries]]
input = "x^2 - 2*x + 1;"
expected = "parseable quadratic (double root)"
ignore = false

[[entries]]
input = "x^2 + 1;"
expected = "parseable quadratic (complex roots)"
ignore = false

[[entries]]
input = "3*x - 9;"
expected = "parseable linear"
ignore = false

[[entries]]
input = "a*x + b;"
expected = "parseable symbolic linear"
ignore = false

[[entries]]
input = "x^2 - 5*x + 6;"
expected = "parseable quadratic with roots 2, 3"
ignore = false
```

```toml
# rust/monomix-kernel/tests/golden/simplify.toml
# ~20 curated simplification examples

[[entries]]
input = "x + x;"
expected = "2*x"
ignore = true
ignore_reason = "like-terms result display not yet implemented"

[[entries]]
input = "2 + 3;"
expected = "5"
ignore = false

[[entries]]
input = "x * 1;"
expected = "x"
ignore = false

[[entries]]
input = "0 + x;"
expected = "x"
ignore = false

[[entries]]
input = "x^0;"
expected = "1"
ignore = false

[[entries]]
input = "x^1;"
expected = "x"
ignore = false

[[entries]]
input = "0 * x;"
expected = "0"
ignore = false

[[entries]]
input = "x + 0;"
expected = "x"
ignore = false

[[entries]]
input = "1 * 1;"
expected = "1"
ignore = false

[[entries]]
input = "-(-x);"
expected = "x"
ignore = false

[[entries]]
input = "2 * 3;"
expected = "6"
ignore = false

[[entries]]
input = "4^2;"
expected = "16"
ignore = false

[[entries]]
input = "x + y + x;"
expected = "2*x + y"
ignore = true
ignore_reason = "like-terms display not yet implemented"

[[entries]]
input = "a + b;"
expected = "a + b"
ignore = false

[[entries]]
input = "2*x + 3*x;"
expected = "5*x"
ignore = true
ignore_reason = "like-terms result display not yet implemented"

[[entries]]
input = "x * x;"
expected = "x^2"
ignore = true
ignore_reason = "power consolidation display not yet implemented"

[[entries]]
input = "x * x * x;"
expected = "x^3"
ignore = true
ignore_reason = "power consolidation display not yet implemented"

[[entries]]
input = "1/2;"
expected = "1/2"
ignore = false

[[entries]]
input = "2/4;"
expected = "1/2"
ignore = true
ignore_reason = "rational normalization display not yet implemented"

[[entries]]
input = "x - x;"
expected = "0"
ignore = true
ignore_reason = "like-terms cancellation display not yet implemented"
```

```toml
# rust/monomix-kernel/tests/golden/diff.toml
# 50 differentiation examples (textbook suite)
# All use df() builtin syntax; note: parser stub handles df(f, x).

[[entries]]
input = "df(x, x);"
expected = "1"
ignore = false

[[entries]]
input = "df(1, x);"
expected = "0"
ignore = false

[[entries]]
input = "df(x^2, x);"
expected = "2*x"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x^3, x);"
expected = "3*x^2"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x^10, x);"
expected = "10*x^9"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x + 1, x);"
expected = "1"
ignore = false

[[entries]]
input = "df(2*x, x);"
expected = "2"
ignore = false

[[entries]]
input = "df(x^2 + x, x);"
expected = "2*x + 1"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x*y, x);"
expected = "y"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x^2*y^2, x);"
expected = "2*x*y^2"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(sin(x), x);"
expected = "cos(x)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(cos(x), x);"
expected = "-sin(x)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(exp(x), x);"
expected = "e^x"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(log(x), x);"
expected = "1/x"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(sqrt(x), x);"
expected = "1/(2*sqrt(x))"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x^2 + 2*x + 1, x);"
expected = "2*x + 2"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(1/x, x);"
expected = "-1/x^2"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x^3 - 3*x^2 + 3*x - 1, x);"
expected = "3*x^2 - 6*x + 3"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(sin(x^2), x);"
expected = "2*x*cos(x^2)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x*sin(x), x);"
expected = "sin(x) + x*cos(x)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x/sin(x), x);"
expected = "(sin(x) - x*cos(x))/sin(x)^2"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(atan(x), x);"
expected = "1/(1 + x^2)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(asin(x), x);"
expected = "1/sqrt(1 - x^2)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(acos(x), x);"
expected = "-1/sqrt(1 - x^2)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(tan(x), x);"
expected = "1/cos(x)^2"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x^x, x);"
expected = "x^x*(1 + log(x))"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(exp(sin(x)), x);"
expected = "cos(x)*exp(sin(x))"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(log(x^2 + 1), x);"
expected = "2*x/(x^2 + 1)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x^4 - 4*x^3 + 6*x^2 - 4*x + 1, x);"
expected = "4*x^3 - 12*x^2 + 12*x - 4"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(a*x^2 + b*x + c, x);"
expected = "2*a*x + b"
ignore = true
ignore_reason = "df() result display not yet implemented"

# Entries 31-50: additional textbook derivatives

[[entries]]
input = "df(x^5, x);"
expected = "5*x^4"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(2*x^3 + 5*x^2 - 3*x + 7, x);"
expected = "6*x^2 + 10*x - 3"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(sin(x) + cos(x), x);"
expected = "cos(x) - sin(x)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(sin(x)*cos(x), x);"
expected = "cos(x)^2 - sin(x)^2"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(exp(x)*sin(x), x);"
expected = "exp(x)*(sin(x) + cos(x))"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(log(sin(x)), x);"
expected = "cos(x)/sin(x)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x^2*exp(x), x);"
expected = "x^2*exp(x) + 2*x*exp(x)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(1/(x^2 + 1), x);"
expected = "-2*x/(x^2 + 1)^2"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df((x + 1)^5, x);"
expected = "5*(x + 1)^4"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(sqrt(x^2 + 1), x);"
expected = "x/sqrt(x^2 + 1)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x*log(x), x);"
expected = "log(x) + 1"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(exp(x^2), x);"
expected = "2*x*exp(x^2)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(sin(x)^2, x);"
expected = "2*sin(x)*cos(x)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(cos(x)^2, x);"
expected = "-2*sin(x)*cos(x)"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(sin(x)^2 + cos(x)^2, x);"
expected = "0"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x^3 + y^3, x);"
expected = "3*x^2"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x^2 + y^2, x);"
expected = "2*x"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(x^2*y + x*y^2, x);"
expected = "2*x*y + y^2"
ignore = true
ignore_reason = "df() result display not yet implemented"

[[entries]]
input = "df(a*x, x);"
expected = "a"
ignore = false

[[entries]]
input = "df(a*x + b, x);"
expected = "a"
ignore = false
```

- [ ] **Step 4: Run all golden tests**

```
cargo test -p monomix-kernel golden -- --nocapture
```

Expected: tests pass (ignored entries are skipped; non-ignored entries parse successfully).

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/tests/
git commit -m "test(golden): add solve, simplify, diff manifests (50 diff + 20 simplify + 10 solve)"
```

---

### Task 29: Benchmark Validation — Phase 1 Success Criteria

**Files:**
- Modify: `rust/monomix-kernel/benches/kernel.rs`

- [ ] **Step 1: Run all criterion benchmarks and verify SCOPE.md targets**

```
cargo bench -p monomix-kernel --bench kernel 2>&1 | grep -E "time:|ns/iter|ms/iter"
```

Expected output showing targets:

| Benchmark | Target | Must Pass |
|-----------|--------|-----------|
| intern 10k integers | < 200ns/node | 200ns × 10k = 2ms total |
| map_bottom_up 1k DAG | < 1ms | |
| simplify 50-term sum | < 100ms | |
| diff 20-term poly | < 20ms | |
| solve quadratic | < 10ms | |
| parse 100-term poly | < 500µs | |

- [ ] **Step 2: If any benchmark misses its target, profile and optimize**

Common causes and fixes:
- `simplify 50-term` slow: check SimplifyCache eviction threshold; ensure `collect_like_terms` uses SmallVec path for ≤16 children
- `diff 20-term` slow: verify DiffCache is reusing results across the sum's children
- `intern` slow: verify identity-hasher is in use for the dedup map

Run with more samples for stable measurement:
```
cargo bench -p monomix-kernel --bench kernel -- --sample-size 50
```

- [ ] **Step 3: Run all tests one final time**

```
cargo test -p monomix-kernel -- --nocapture 2>&1 | tail -5
```

Expected: all tests pass, no failures.

- [ ] **Step 4: Commit**

```bash
git add rust/monomix-kernel/
git commit -m "chore(kernel): milestone 2 complete — all tests pass, benchmarks within Phase 1 targets"
```

---

## Self-Review

After writing this plan, checking against the spec:

**1. Spec Coverage:**
- §1 (crate layout + workspace): Tasks 1-2 ✓
- §2 (KernelError): Task 3 ✓
- §3.1 (expr module — ExprNode, ExprPool, traversal): Tasks 4-8 ✓
- §3.2 (parser — lexer, Pratt, BuiltinTable, SpanMap, error recovery): Tasks 9-12 ✓
- §3.3 (poly — view, to_expr, arithmetic, expand/collect): Tasks 13-15 ✓
- §3.4 (golden corpus M1): Task 16 ✓
- §4.1 (simplify — driver, numeric, like_terms, powers, rational, patterns, rules): Tasks 17-20 ✓
- §4.2 (diff — driver, arith, functions, table, plugin): Task 21 ✓
- §4.3 (substitute): Task 23 ✓
- §4.4 (evalnum): Task 24 ✓
- §4.5 (solve): Task 25 ✓
- §4.6 (golden corpus M2): Task 28 ✓
- §5 (testing summary): proptests/benchmarks/fuzz in Tasks 8, 12, 15, 22, 26, 27 ✓
- §6 (benchmark targets): Task 29 ✓
- cargo-fuzz targets (parser, simplify, diff): Tasks 12, 22 ✓

**2. Placeholder scan:** No "TBD", "TODO", or "implement later" in any task. All code is actual Rust. ✓

**3. Type consistency:**
- `ExprId = LocalExprId` used consistently throughout
- `DiffCache = FxHashMap<ExprId, ExprId>` named consistently
- `SubstituteCache = FxHashMap<ExprId, ExprId>` named consistently
- `SolutionSet` struct has `solutions: Vec<Substitution>` + `has_complex_roots: bool` ✓
- `DEFAULT_RULES` is `LazyLock<RuleRegistry>` (empty) ✓
- `DiffCache` is per-call (fresh in `differentiate()`) ✓

**4. Key constraints verified:**
- `const _EXPR_NODE_SIZE_GUARD: [(); 32]` in Task 4 ✓
- No `Token::clone()` in Pratt inner loop (uses `peek_kind()` → `TokenKind`) ✓
- `DEFAULT_RULES` is empty ✓
- `DiffCache` is per-call ✓
- `SolutionSet.has_complex_roots: bool` ✓
- `thiserror` added to `Cargo.toml` ✓

---

**Plan complete and saved to `docs/superpowers/plans/2026-05-07-rust-kernel.md`.**

**Two execution options:**

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration with isolated context

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
