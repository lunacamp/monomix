# Python Bindings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the Rust `monomix-kernel` to Python via PyO3 + maturin, replacing the placeholder dataclass IR with a Rust-backed `monomix.Expr`. Includes `Session`, full operator overloading, kernel ExprNode extensions for boolean/comparison nodes, error mapping, GIL release, and SMT bridge rewrite against the new IR.

**Architecture:** Three tiers — Python surface (`python/monomix/`), PyO3 boundary (new `rust/monomix-py/` crate), and existing pure-Rust kernel (`rust/monomix-kernel/`). Pool-per-Session ownership via `Arc<Mutex<ExprPool>>`; `Expr` is an opaque handle. Operator overloads cover arithmetic (`+ - * / **`), comparison (`== != < <= > >=`), and boolean (`& | ~`); each comparison/boolean op builds a kernel node. The SMT bridge consumes the new Expr through a small inspection API.

**Tech Stack:** Rust + PyO3 + maturin; Python 3.11+; pytest + hypothesis; pyright (strict); existing kernel with `ExprPool` / `ExprId` arena model. Reference spec: `docs/superpowers/specs/2026-05-13-python-bindings-design.md`.

---

## File Structure

### Files to create

| Path | Responsibility |
|------|----------------|
| `rust/monomix-py/Cargo.toml` | New PyO3 crate manifest |
| `rust/monomix-py/src/lib.rs` | `#[pymodule]` entry point |
| `rust/monomix-py/src/session.rs` | `_SessionHandle` pyclass owning `Arc<Mutex<ExprPool>>` |
| `rust/monomix-py/src/expr.rs` | `Expr` pyclass + operator overloading |
| `rust/monomix-py/src/errors.rs` | `KernelError → PyErr` mapping |
| `rust/monomix-py/src/kernel_fns.rs` | Module-level `parse`, `simplify`, `df`, etc. |
| `python/monomix/errors.py` | `MonomixError` hierarchy |
| `python/monomix/session.py` | Python `Session` class wrapping `_SessionHandle` |
| `python/monomix/_kernel.pyi` | Type stubs for pyright strict |
| `python/monomix/smt/__init__.py` | Public SMT facade (renamed from `solver/`) |
| `python/monomix/smt/errors.py` | SMT-specific exceptions |
| `python/monomix/smt/translate.py` | Walker over new Rust-backed Expr |
| `python/monomix/smt/z3_backend.py` | Z3-specific backend (moved + updated) |
| `python/tests/test_expr.py` | Operator overloading tests |
| `python/tests/test_session.py` | Session lifetime and bindings tests |
| `python/tests/test_kernel_calls.py` | Module-level function tests |
| `python/tests/test_gil.py` | GIL-release / concurrency soft-floor test |
| `python/tests/test_smt.py` | SMT bridge tests (replaces `test_solver.py`) |
| `docs/python-bindings.md` | User-facing docs page |

### Files to modify

| Path | Change |
|------|--------|
| `Cargo.toml` (workspace root) | Add `rust/monomix-py` to `members` |
| `rust/monomix-kernel/src/expr/mod.rs` | New ExprNode variants + constructors + match arms |
| `rust/monomix-kernel/src/diff/driver.rs` | Add UnsupportedError arms for new variants |
| `rust/monomix-kernel/src/evalnum/mod.rs` | Add UnsupportedError arms for new variants |
| `python/pyproject.toml` | Switch build-backend from setuptools to maturin |
| `python/monomix/__init__.py` | Re-export new public API |
| `CLAUDE.md` | Update "only active code" section (slice 8) |

### Files to delete

| Path | Reason |
|------|--------|
| `python/monomix/expr.py` | Placeholder dataclass IR, replaced by Rust-backed `Expr` |
| `python/monomix/solver/` (whole directory) | Renamed to `monomix/smt/` and rewritten |
| `python/tests/test_solver.py` | Replaced by `test_smt.py` |

---

## Phase 1: Kernel ExprNode extensions

Adds `Lt`, `Le`, `Gt`, `Ge`, `Not`, `And`, `Or`, `Implies`, `BoolConst` variants. Pure Rust work — no Python in this phase. Each task ends with `cargo test --lib` green from `rust/monomix-kernel/`.

### Task 1.1: Add comparison and boolean variants to ExprNode

**Files:**
- Modify: `rust/monomix-kernel/src/expr/mod.rs:42-60` (the `ExprNode` enum)

- [ ] **Step 1: Write the failing test (size guard + variant existence)**

Append to `rust/monomix-kernel/src/expr/mod.rs` inside `mod tests`:

```rust
#[test]
fn new_variants_exist_and_intern() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let y = pool.symbol("y");
    let _ = pool.lt(x, y);
    let _ = pool.le(x, y);
    let _ = pool.gt(x, y);
    let _ = pool.ge(x, y);
    let _ = pool.not_node(x);
    let _ = pool.and_(vec![x, y]);
    let _ = pool.or_(vec![x, y]);
    let _ = pool.implies(x, y);
    let _ = pool.bool_const(true);
    let _ = pool.bool_const(false);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd rust/monomix-kernel && cargo test --lib new_variants_exist_and_intern
```

Expected: FAIL — `pool.lt` / `pool.le` / etc. don't exist.

- [ ] **Step 3: Add the variants to the enum**

In `rust/monomix-kernel/src/expr/mod.rs`, extend the `ExprNode` enum:

```rust
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

    // Comparison
    Lt(ExprId, ExprId),
    Le(ExprId, ExprId),
    Gt(ExprId, ExprId),
    Ge(ExprId, ExprId),

    // Propositional
    Not(ExprId),
    And(Box<[ExprId]>),
    Or(Box<[ExprId]>),
    Implies(ExprId, ExprId),
    BoolConst(bool),
}
```

- [ ] **Step 4: Verify size guard still holds**

```bash
cargo build --lib
```

Expected: compile success. The `_EXPR_NODE_SIZE_GUARD` const assert will fail compilation if any variant pushes the enum past 32 bytes. If it fails, the most likely culprit is that adding `BoolConst(bool)` somehow pushed alignment — unlikely, but if so, box the bool: `BoolConst(Box<bool>)`. The compile-error message will be explicit.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/expr/mod.rs
git commit -m "Add comparison and boolean variants to ExprNode"
```

### Task 1.2: Update internal helpers to handle new variants

**Files:**
- Modify: `rust/monomix-kernel/src/expr/mod.rs` — `content_hash`, `subtree_size_of`, `children`, `is_atom`, `fold_impl`, `map_bottom_up`

- [ ] **Step 1: Write failing tests for the helpers**

Append to `mod tests`:

```rust
#[test]
fn new_variants_subtree_size() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let y = pool.symbol("y");
    let lt = pool.lt(x, y);
    assert_eq!(pool.subtree_size(lt), 3); // Lt + x + y

    let and_node = pool.and_(vec![x, y]);
    assert_eq!(pool.subtree_size(and_node), 3);

    let nt = pool.not_node(x);
    assert_eq!(pool.subtree_size(nt), 2);

    let bc = pool.bool_const(true);
    assert_eq!(pool.subtree_size(bc), 1);
}

#[test]
fn new_variants_children() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let y = pool.symbol("y");
    let lt = pool.lt(x, y);
    assert_eq!(pool.children(lt), vec![x, y]);

    let and_node = pool.and_(vec![x, y]);
    assert_eq!(pool.children(and_node), vec![x, y]);

    let nt = pool.not_node(x);
    assert_eq!(pool.children(nt), vec![x]);

    let bc = pool.bool_const(false);
    assert_eq!(pool.children(bc), Vec::<ExprId>::new());
}

#[test]
fn bool_const_is_atom() {
    let mut pool = ExprPool::new();
    let bc = pool.bool_const(true);
    assert!(pool.is_atom(bc));
    assert!(!pool.is_numeric(bc));
}

#[test]
fn map_bottom_up_recurses_into_new_variants() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let y = pool.symbol("y");
    let lt = pool.lt(x, y);
    let result = pool.map_bottom_up_fresh(lt, &mut |_pool, id| id);
    assert_eq!(result, lt);

    let and_node = pool.and_(vec![x, y]);
    let result = pool.map_bottom_up_fresh(and_node, &mut |_pool, id| id);
    assert_eq!(result, and_node);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib new_variants
```

Expected: tests don't even compile yet — `pool.lt` etc. don't have implementations (only added to the enum).

- [ ] **Step 3: Extend `content_hash`**

In `rust/monomix-kernel/src/expr/mod.rs`, find `fn content_hash` and add arms:

```rust
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
        ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c)
        | ExprNode::And(c) | ExprNode::Or(c) => {
            for id in c.iter() { id.hash(&mut h); }
        }
        ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b)
        | ExprNode::Lt(a, b) | ExprNode::Le(a, b)
        | ExprNode::Gt(a, b) | ExprNode::Ge(a, b)
        | ExprNode::Implies(a, b) => {
            a.hash(&mut h); b.hash(&mut h);
        }
        ExprNode::Neg(x) | ExprNode::Not(x) => x.hash(&mut h),
        ExprNode::Fn(tag, args) => {
            tag.hash(&mut h);
            for id in args.iter() { id.hash(&mut h); }
        }
        ExprNode::BoolConst(b) => b.hash(&mut h),
    }
    h.finish()
}
```

- [ ] **Step 4: Extend `subtree_size_of`**

```rust
fn subtree_size_of(node: &ExprNode, nodes: &[ArenaEntry]) -> u32 {
    let children: &[ExprId] = match node {
        ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c)
        | ExprNode::And(c) | ExprNode::Or(c) => c,
        ExprNode::Fn(_, c) => c,
        ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b)
        | ExprNode::Lt(a, b) | ExprNode::Le(a, b)
        | ExprNode::Gt(a, b) | ExprNode::Ge(a, b)
        | ExprNode::Implies(a, b) => {
            return 1 + nodes[a.0 as usize].subtree_size
                     + nodes[b.0 as usize].subtree_size;
        }
        ExprNode::Neg(x) | ExprNode::Not(x) => return 1 + nodes[x.0 as usize].subtree_size,
        _ => return 1,
    };
    1 + children.iter().map(|c| nodes[c.0 as usize].subtree_size).sum::<u32>()
}
```

- [ ] **Step 5: Extend `children`**

```rust
pub fn children(&self, id: ExprId) -> Vec<ExprId> {
    match &self.nodes[id.0 as usize].node {
        ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c)
        | ExprNode::And(c) | ExprNode::Or(c) => c.to_vec(),
        ExprNode::Fn(_, c) => c.to_vec(),
        ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b)
        | ExprNode::Lt(a, b) | ExprNode::Le(a, b)
        | ExprNode::Gt(a, b) | ExprNode::Ge(a, b)
        | ExprNode::Implies(a, b) => vec![*a, *b],
        ExprNode::Neg(x) | ExprNode::Not(x) => vec![*x],
        _ => Vec::new(),
    }
}
```

- [ ] **Step 6: Extend `is_atom`**

```rust
pub fn is_atom(&self, id: ExprId) -> bool {
    matches!(self.get(id),
        ExprNode::SmallInt(_) | ExprNode::BigInt(_) | ExprNode::Rational(_)
        | ExprNode::Float(_) | ExprNode::Symbol(_) | ExprNode::String(_)
        | ExprNode::BoolConst(_))
}
```

- [ ] **Step 7: Extend `fold_impl`**

In `fold_impl`, add arms after the existing composite handlers:

```rust
let acc = match node {
    ExprNode::Add(c) | ExprNode::Mul(c) | ExprNode::List(c)
    | ExprNode::And(c) | ExprNode::Or(c) => {
        let ids: Vec<ExprId> = c.to_vec();
        ids.iter().fold(acc, |a, &child| self.fold_impl(child, a, f, visited))
    }
    ExprNode::Fn(_, c) => {
        let ids: Vec<ExprId> = c.to_vec();
        ids.iter().fold(acc, |a, &child| self.fold_impl(child, a, f, visited))
    }
    ExprNode::Pow(a, b) | ExprNode::Div(a, b) | ExprNode::Eq(a, b)
    | ExprNode::Lt(a, b) | ExprNode::Le(a, b)
    | ExprNode::Gt(a, b) | ExprNode::Ge(a, b)
    | ExprNode::Implies(a, b) => {
        let (a, b) = (*a, *b);
        let acc = self.fold_impl(a, acc, f, visited);
        self.fold_impl(b, acc, f, visited)
    }
    ExprNode::Neg(x) | ExprNode::Not(x) => { let x = *x; self.fold_impl(x, acc, f, visited) }
    _ => acc,
};
```

- [ ] **Step 8: Extend `map_bottom_up`**

Add arms in the `match node` block (after `ExprNode::Eq` arm):

```rust
ExprNode::Lt(a, b) => {
    let a2 = self.map_bottom_up(a, cache, f);
    let b2 = self.map_bottom_up(b, cache, f);
    self.lt(a2, b2)
}
ExprNode::Le(a, b) => {
    let a2 = self.map_bottom_up(a, cache, f);
    let b2 = self.map_bottom_up(b, cache, f);
    self.le(a2, b2)
}
ExprNode::Gt(a, b) => {
    let a2 = self.map_bottom_up(a, cache, f);
    let b2 = self.map_bottom_up(b, cache, f);
    self.gt(a2, b2)
}
ExprNode::Ge(a, b) => {
    let a2 = self.map_bottom_up(a, cache, f);
    let b2 = self.map_bottom_up(b, cache, f);
    self.ge(a2, b2)
}
ExprNode::Not(x) => {
    let x2 = self.map_bottom_up(x, cache, f);
    self.not_node(x2)
}
ExprNode::And(c) => {
    let ids: Vec<ExprId> = c.iter().map(|&child| self.map_bottom_up(child, cache, f)).collect();
    self.and_(ids)
}
ExprNode::Or(c) => {
    let ids: Vec<ExprId> = c.iter().map(|&child| self.map_bottom_up(child, cache, f)).collect();
    self.or_(ids)
}
ExprNode::Implies(a, b) => {
    let a2 = self.map_bottom_up(a, cache, f);
    let b2 = self.map_bottom_up(b, cache, f);
    self.implies(a2, b2)
}
ExprNode::BoolConst(_) => root,
```

- [ ] **Step 9: Add constructor methods**

Append to the `impl ExprPool` block (after `pub fn list`):

```rust
pub fn lt(&mut self, a: ExprId, b: ExprId) -> ExprId {
    self.intern(ExprNode::Lt(a, b))
}

pub fn le(&mut self, a: ExprId, b: ExprId) -> ExprId {
    self.intern(ExprNode::Le(a, b))
}

pub fn gt(&mut self, a: ExprId, b: ExprId) -> ExprId {
    self.intern(ExprNode::Gt(a, b))
}

pub fn ge(&mut self, a: ExprId, b: ExprId) -> ExprId {
    self.intern(ExprNode::Ge(a, b))
}

pub fn not_node(&mut self, x: ExprId) -> ExprId {
    if let ExprNode::Not(inner) = *self.get(x) {
        return inner; // not(not(x)) → x
    }
    if let ExprNode::BoolConst(b) = *self.get(x) {
        return self.bool_const(!b);
    }
    self.intern(ExprNode::Not(x))
}

pub fn and_(&mut self, children: Vec<ExprId>) -> ExprId {
    // Flatten nested And nodes
    let mut flat: Vec<ExprId> = Vec::with_capacity(children.len());
    for c in children {
        if let ExprNode::And(inner) = self.get(c).clone() {
            flat.extend_from_slice(&inner);
        } else {
            flat.push(c);
        }
    }
    // Short-circuit on false; drop true
    let true_id = self.bool_const(true);
    let false_id = self.bool_const(false);
    if flat.iter().any(|&c| c == false_id) {
        return false_id;
    }
    flat.retain(|&c| c != true_id);
    if flat.is_empty() {
        return true_id;
    }
    if flat.len() == 1 {
        return flat[0];
    }
    flat.sort_unstable();
    flat.dedup();
    self.intern(ExprNode::And(flat.into_boxed_slice()))
}

pub fn or_(&mut self, children: Vec<ExprId>) -> ExprId {
    let mut flat: Vec<ExprId> = Vec::with_capacity(children.len());
    for c in children {
        if let ExprNode::Or(inner) = self.get(c).clone() {
            flat.extend_from_slice(&inner);
        } else {
            flat.push(c);
        }
    }
    let true_id = self.bool_const(true);
    let false_id = self.bool_const(false);
    if flat.iter().any(|&c| c == true_id) {
        return true_id;
    }
    flat.retain(|&c| c != false_id);
    if flat.is_empty() {
        return false_id;
    }
    if flat.len() == 1 {
        return flat[0];
    }
    flat.sort_unstable();
    flat.dedup();
    self.intern(ExprNode::Or(flat.into_boxed_slice()))
}

pub fn implies(&mut self, a: ExprId, b: ExprId) -> ExprId {
    self.intern(ExprNode::Implies(a, b))
}

pub fn bool_const(&mut self, b: bool) -> ExprId {
    self.intern(ExprNode::BoolConst(b))
}
```

- [ ] **Step 10: Run all tests to verify**

```bash
cargo test --lib
```

Expected: all green, including the new `new_variants_*` tests and the existing tests.

- [ ] **Step 11: Commit**

```bash
git add rust/monomix-kernel/src/expr/mod.rs
git commit -m "Extend ExprPool internals for new ExprNode variants"
```

### Task 1.3: Add constant-folding tests

**Files:**
- Modify: `rust/monomix-kernel/src/expr/mod.rs` — `mod tests`

- [ ] **Step 1: Write the failing tests**

Append to `mod tests`:

```rust
#[test]
fn not_double_negation() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let not_x = pool.not_node(x);
    let not_not_x = pool.not_node(not_x);
    assert_eq!(not_not_x, x);
}

#[test]
fn not_of_bool_const_folds() {
    let mut pool = ExprPool::new();
    let t = pool.bool_const(true);
    let f = pool.bool_const(false);
    assert_eq!(pool.not_node(t), f);
    assert_eq!(pool.not_node(f), t);
}

#[test]
fn and_short_circuits_on_false() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let f = pool.bool_const(false);
    assert_eq!(pool.and_(vec![x, f]), f);
}

#[test]
fn and_drops_true_operands() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let t = pool.bool_const(true);
    assert_eq!(pool.and_(vec![x, t]), x);
}

#[test]
fn or_short_circuits_on_true() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let t = pool.bool_const(true);
    assert_eq!(pool.or_(vec![x, t]), t);
}

#[test]
fn or_drops_false_operands() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let f = pool.bool_const(false);
    assert_eq!(pool.or_(vec![x, f]), x);
}

#[test]
fn and_flattens() {
    let mut pool = ExprPool::new();
    let a = pool.symbol("a");
    let b = pool.symbol("b");
    let c = pool.symbol("c");
    let ab = pool.and_(vec![a, b]);
    let abc = pool.and_(vec![ab, c]);
    let expected = pool.and_(vec![a, b, c]);
    assert_eq!(abc, expected);
}

#[test]
fn and_sorts_and_dedups() {
    let mut pool = ExprPool::new();
    let a = pool.symbol("a");
    let b = pool.symbol("b");
    let ab = pool.and_(vec![a, b, a]);
    let ba = pool.and_(vec![b, a]);
    assert_eq!(ab, ba);
}

#[test]
fn bool_const_interning_idempotent() {
    let mut pool = ExprPool::new();
    let t1 = pool.bool_const(true);
    let t2 = pool.bool_const(true);
    assert_eq!(t1, t2);
    let f1 = pool.bool_const(false);
    assert_ne!(t1, f1);
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --lib
```

Expected: all PASS — the constructors from Task 1.2 already implement the folding.

- [ ] **Step 3: Commit**

```bash
git add rust/monomix-kernel/src/expr/mod.rs
git commit -m "Cover ExprPool constant folding for new variants"
```

### Task 1.4: Surface UnsupportedError from differentiator for new variants

**Files:**
- Modify: `rust/monomix-kernel/src/diff/driver.rs`

- [ ] **Step 1: Read the file to find the dispatch site**

```bash
grep -n "match" rust/monomix-kernel/src/diff/driver.rs | head -20
```

Locate the `match` over `ExprNode` that drives differentiation.

- [ ] **Step 2: Write a failing test**

Append to `rust/monomix-kernel/src/diff/mod.rs` (or `driver.rs` `mod tests` if that's where existing tests live):

```rust
#[test]
fn differentiate_lt_is_unsupported() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let y = pool.symbol("y");
    let lt = pool.lt(x, y);
    let result = differentiate(&mut pool, lt, x);
    assert!(matches!(result, Err(KernelError::Unsupported { .. }) | Err(_)));
}
```

(If the existing differentiator panics on unknown variants instead of returning Err, the test will fail differently and the next step's fix prevents the panic.)

- [ ] **Step 3: Run test**

```bash
cargo test --lib differentiate_lt_is_unsupported
```

Expected: FAIL (panic or wrong error variant).

- [ ] **Step 4: Add a "boolean / comparison nodes are not differentiable" arm**

In the differentiator dispatch (likely a `match` on `ExprNode` kind), add an arm — pattern depends on the existing code. If the existing structure has explicit arms with `_ => panic!(...)`, change the default to:

```rust
ExprNode::Lt(_, _) | ExprNode::Le(_, _) | ExprNode::Gt(_, _) | ExprNode::Ge(_, _)
| ExprNode::Not(_) | ExprNode::And(_) | ExprNode::Or(_) | ExprNode::Implies(_, _)
| ExprNode::BoolConst(_) => Err(KernelError::UnsupportedEquation {
    reason: "boolean/comparison expressions are not differentiable".to_string(),
}),
```

(Use whichever existing `KernelError` variant best matches; if none does, add a new variant `KernelError::NonDifferentiable { reason: String }` in `error.rs`. Prefer reusing existing variants to keep the error enum focused.)

- [ ] **Step 5: Run tests**

```bash
cargo test --lib
```

Expected: green.

- [ ] **Step 6: Commit**

```bash
git add rust/monomix-kernel/src/diff/ rust/monomix-kernel/src/error.rs
git commit -m "Return UnsupportedError when differentiating boolean/comparison nodes"
```

### Task 1.5: Surface UnsupportedError from evalnum for new variants

**Files:**
- Modify: `rust/monomix-kernel/src/evalnum/mod.rs`

- [ ] **Step 1: Write a failing test**

Append to `rust/monomix-kernel/src/evalnum/mod.rs` (or wherever evalnum tests live):

```rust
#[test]
fn evalnum_bool_const_is_unsupported() {
    let mut pool = ExprPool::new();
    let t = pool.bool_const(true);
    let result = evaluate_numeric(&pool, t, &Default::default());
    assert!(result.is_err());
}

#[test]
fn evalnum_and_is_unsupported() {
    let mut pool = ExprPool::new();
    let x = pool.symbol("x");
    let y = pool.symbol("y");
    let and_node = pool.and_(vec![x, y]);
    let result = evaluate_numeric(&pool, and_node, &Default::default());
    assert!(result.is_err());
}
```

(Adjust the `evaluate_numeric` call shape to match the actual function signature — read `evalnum/mod.rs` first.)

- [ ] **Step 2: Run tests**

```bash
cargo test --lib evalnum
```

Expected: FAIL (panic or unexpected behavior).

- [ ] **Step 3: Add UnsupportedError arms**

In the `evaluate_numeric` dispatch, add explicit arms returning `Err(KernelError::UnsupportedFn)` (or a more specific variant) for `Lt`, `Le`, `Gt`, `Ge`, `Not`, `And`, `Or`, `Implies`, `BoolConst`.

- [ ] **Step 4: Run tests**

```bash
cargo test --lib
```

Expected: green.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-kernel/src/evalnum/
git commit -m "Return UnsupportedError from evalnum for boolean/comparison nodes"
```

### Task 1.6: Verify all other ExprNode match sites still compile

**Files:**
- Inspect: `solve/`, `substitute/`, `simplify/`, `poly/`, `parser/expr.rs` (all of which match on ExprNode)

- [ ] **Step 1: Compile the whole kernel**

```bash
cd rust/monomix-kernel && cargo build --lib --all-targets
```

Expected: compile. Most sites use a `_` default arm so adding variants doesn't break them. If any site is exhaustive and fails to compile, the error message will name the file and line.

- [ ] **Step 2: If any file fails compilation, add the new variants to its match**

Apply the same `Lt | Le | Gt | Ge | Not | And | Or | Implies | BoolConst => ...` pattern with whatever behavior the rest of that file's default arm currently has (typically pass through unchanged or return an `Unsupported` error). Each fix is one file.

- [ ] **Step 3: Run all kernel tests**

```bash
cargo test
```

Expected: every test green, including `cargo test --test golden_tests` (no goldens should change because the parser surface hasn't grown).

- [ ] **Step 4: Run clippy**

```bash
cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 5: Commit (if any fixes were needed)**

```bash
git add rust/monomix-kernel/
git commit -m "Cover new ExprNode variants in remaining kernel match sites"
```

If no fixes were needed, skip the commit.

---

## Phase 2: Workspace + walking skeleton

Add `rust/monomix-py/` crate. Switch `python/pyproject.toml` to maturin. Verify `maturin develop` builds a Python extension exposing one trivial function.

### Task 2.1: Add `rust/monomix-py/` crate manifest

**Files:**
- Create: `rust/monomix-py/Cargo.toml`

- [ ] **Step 1: Create the manifest**

```toml
[package]
name        = "monomix-py"
version     = "0.1.0"
edition.workspace = true
license.workspace = true
authors.workspace = true

[lib]
name        = "monomix_py"
crate-type  = ["cdylib"]

[dependencies]
pyo3              = { version = "0.22", features = ["extension-module", "num-bigint", "abi3-py311"] }
num-bigint        = "0.4"
num-rational      = "0.4"
num-traits        = "0.2"
monomix-kernel    = { path = "../monomix-kernel" }

[lints]
workspace = true
```

- [ ] **Step 2: Verify the manifest is syntactically valid**

```bash
cd rust/monomix-py && cargo check --no-default-features 2>&1 | head -20
```

Expected: errors about missing `src/lib.rs`. Manifest itself parses.

- [ ] **Step 3: Commit**

```bash
git add rust/monomix-py/Cargo.toml
git commit -m "Add monomix-py crate manifest"
```

### Task 2.2: Add `rust/monomix-py/` to the workspace

**Files:**
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Update workspace members**

In `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members  = ["rust/solver-bridge", "rust/monomix-kernel", "rust/monomix-py"]
```

- [ ] **Step 2: Commit**

```bash
git add Cargo.toml
git commit -m "Register monomix-py crate in workspace"
```

### Task 2.3: Write minimal `#[pymodule]` exposing `__version__`

**Files:**
- Create: `rust/monomix-py/src/lib.rs`

- [ ] **Step 1: Write the module skeleton**

```rust
use pyo3::prelude::*;

#[pymodule]
fn _kernel(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
```

- [ ] **Step 2: Build the crate**

```bash
cd rust/monomix-py && cargo build --lib
```

Expected: compile.

- [ ] **Step 3: Commit**

```bash
git add rust/monomix-py/src/lib.rs
git commit -m "Add walking-skeleton pymodule for monomix-py"
```

### Task 2.4: Switch `python/pyproject.toml` to maturin

**Files:**
- Modify: `python/pyproject.toml`

- [ ] **Step 1: Rewrite the pyproject**

```toml
[build-system]
requires = ["maturin>=1.7,<2.0"]
build-backend = "maturin"

[project]
name = "monomix"
version = "0.0.1"
description = "Modern CAS rewrite of REDUCE — Python facade and SMT bridge"
requires-python = ">=3.11"
authors = [{ name = "Roman" }]
license = { text = "MIT" }

dependencies = []

[project.optional-dependencies]
smt = ["z3-solver>=4.13"]
dev = [
  "z3-solver>=4.13",
  "pytest>=8",
  "pytest-benchmark>=4",
  "hypothesis>=6",
  "pyright>=1.1",
  "ruff>=0.6",
]

[tool.maturin]
manifest-path = "../rust/monomix-py/Cargo.toml"
python-source = "."
module-name = "monomix._kernel"
features = ["pyo3/extension-module"]
```

- [ ] **Step 2: Install maturin if needed**

```bash
pip install --upgrade maturin
```

- [ ] **Step 3: Build the extension in place**

```bash
cd python && maturin develop
```

Expected: builds the Rust crate, copies the extension into `python/monomix/_kernel*.{so,pyd,dylib}`, installs the package in editable mode.

- [ ] **Step 4: Verify import**

```bash
python -c "import monomix._kernel; print(monomix._kernel.__version__)"
```

Expected: prints `0.1.0`.

- [ ] **Step 5: Commit**

```bash
git add python/pyproject.toml
git commit -m "Switch monomix Python build to maturin"
```

---

## Phase 3: Session + opaque Expr

Introduce `_SessionHandle` and `Expr` pyclasses. No operators yet — only constructors, `__repr__`, cross-session guard, and pool lifetime. Build out the first `.pyi`.

### Task 3.1: Add the error mapping crate module

**Files:**
- Create: `rust/monomix-py/src/errors.rs`
- Modify: `rust/monomix-py/src/lib.rs`

- [ ] **Step 1: Create the errors module**

`rust/monomix-py/src/errors.rs`:

```rust
use monomix_kernel::KernelError;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

create_exception!(monomix._kernel, MonomixError, PyException);
create_exception!(monomix._kernel, ParseError, MonomixError);
create_exception!(monomix._kernel, EvalError, MonomixError);
create_exception!(monomix._kernel, UnsupportedError, MonomixError);
create_exception!(monomix._kernel, CrossSessionError, MonomixError);

pub fn map_kernel_error(err: KernelError) -> PyErr {
    match err {
        KernelError::Parse(diags) => {
            let msg = diags
                .iter()
                .map(|d| format!("{:?}", d))
                .collect::<Vec<_>>()
                .join("; ");
            PyErr::new::<ParseError, _>(msg)
        }
        KernelError::DivisionByZero { .. } => {
            PyErr::new::<EvalError, _>("division by zero")
        }
        KernelError::IndeterminateForm => {
            PyErr::new::<EvalError, _>("indeterminate form 0/0")
        }
        KernelError::UnboundSymbol(name) => {
            PyErr::new::<EvalError, _>(format!("unbound symbol: {}", name))
        }
        KernelError::LogOfNonPositive => {
            PyErr::new::<EvalError, _>("log of non-positive value")
        }
        KernelError::SqrtOfNegative => {
            PyErr::new::<EvalError, _>("sqrt of negative value")
        }
        KernelError::DomainError { fn_name } => {
            PyErr::new::<EvalError, _>(format!("domain error in {}", fn_name))
        }
        KernelError::UnsupportedFn => {
            PyErr::new::<UnsupportedError, _>("unsupported function for numeric eval")
        }
        KernelError::UnsupportedEquation { reason } => {
            PyErr::new::<UnsupportedError, _>(reason)
        }
        KernelError::SingularSystem => {
            PyErr::new::<EvalError, _>("singular system")
        }
        KernelError::Overflow => PyErr::new::<EvalError, _>("arithmetic overflow"),
        KernelError::NumericNaN => PyErr::new::<EvalError, _>("numeric evaluation produced NaN"),
        KernelError::DifferentiateEquation => {
            PyErr::new::<UnsupportedError, _>("cannot differentiate an equation")
        }
        KernelError::NotASymbol => {
            PyErr::new::<EvalError, _>("differentiation variable must be a symbol")
        }
        KernelError::SubstituteNotASymbol => {
            PyErr::new::<EvalError, _>("substitution target must be a symbol")
        }
        KernelError::CyclicBinding => {
            PyErr::new::<EvalError, _>("cyclic binding detected")
        }
        KernelError::PoolExhausted => {
            PyErr::new::<MonomixError, _>("expression pool exhausted")
        }
    }
}
```

- [ ] **Step 2: Add the module declaration to `lib.rs` and register exceptions**

Update `rust/monomix-py/src/lib.rs`:

```rust
use pyo3::prelude::*;

mod errors;

#[pymodule]
fn _kernel(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("MonomixError", m.py().get_type_bound::<errors::MonomixError>())?;
    m.add("ParseError", m.py().get_type_bound::<errors::ParseError>())?;
    m.add("EvalError", m.py().get_type_bound::<errors::EvalError>())?;
    m.add("UnsupportedError", m.py().get_type_bound::<errors::UnsupportedError>())?;
    m.add("CrossSessionError", m.py().get_type_bound::<errors::CrossSessionError>())?;
    Ok(())
}
```

- [ ] **Step 3: Rebuild and verify the exceptions are exposed**

```bash
cd python && maturin develop && python -c "from monomix._kernel import MonomixError, ParseError, EvalError, UnsupportedError, CrossSessionError; print('ok')"
```

Expected: prints `ok`.

- [ ] **Step 4: Commit**

```bash
git add rust/monomix-py/src/errors.rs rust/monomix-py/src/lib.rs
git commit -m "Add KernelError -> Python exception mapping"
```

### Task 3.2: Add the Python error module

**Files:**
- Create: `python/monomix/errors.py`

- [ ] **Step 1: Re-export the kernel-defined exceptions**

```python
"""Public exception hierarchy for monomix.

The actual exception classes are defined by the Rust binding crate
(via PyO3's `create_exception!`). This module re-exports them under
a stable Python import path so user code can write
`from monomix.errors import MonomixError`.
"""

from __future__ import annotations

from monomix._kernel import (
    CrossSessionError,
    EvalError,
    MonomixError,
    ParseError,
    UnsupportedError,
)

__all__ = [
    "MonomixError",
    "ParseError",
    "EvalError",
    "UnsupportedError",
    "CrossSessionError",
]
```

- [ ] **Step 2: Verify**

```bash
python -c "from monomix.errors import MonomixError; print('ok')"
```

Expected: prints `ok`.

- [ ] **Step 3: Commit**

```bash
git add python/monomix/errors.py
git commit -m "Add monomix.errors module re-exporting the exception hierarchy"
```

### Task 3.3: Add `_SessionHandle` pyclass

**Files:**
- Create: `rust/monomix-py/src/session.rs`
- Modify: `rust/monomix-py/src/lib.rs`

- [ ] **Step 1: Write the session-handle pyclass**

`rust/monomix-py/src/session.rs`:

```rust
use monomix_kernel::ExprPool;
use pyo3::prelude::*;
use std::sync::{Arc, Mutex};

#[pyclass(name = "_SessionHandle", module = "monomix._kernel")]
pub struct SessionHandle {
    pub pool: Arc<Mutex<ExprPool>>,
}

#[pymethods]
impl SessionHandle {
    #[new]
    fn new() -> Self {
        SessionHandle {
            pool: Arc::new(Mutex::new(ExprPool::new())),
        }
    }
}

impl SessionHandle {
    pub fn pool_clone(&self) -> Arc<Mutex<ExprPool>> {
        Arc::clone(&self.pool)
    }
}
```

- [ ] **Step 2: Register the class**

In `rust/monomix-py/src/lib.rs`, add `mod session;` near the top and register inside `#[pymodule]`:

```rust
mod session;
// ... in the pymodule body:
m.add_class::<session::SessionHandle>()?;
```

- [ ] **Step 3: Rebuild and smoke-test**

```bash
cd python && maturin develop && python -c "from monomix._kernel import _SessionHandle; s = _SessionHandle(); print('ok')"
```

Expected: prints `ok`.

- [ ] **Step 4: Commit**

```bash
git add rust/monomix-py/src/session.rs rust/monomix-py/src/lib.rs
git commit -m "Add _SessionHandle pyclass owning an Arc<Mutex<ExprPool>>"
```

### Task 3.4: Add the `Expr` pyclass with cross-session guard

**Files:**
- Create: `rust/monomix-py/src/expr.rs`
- Modify: `rust/monomix-py/src/lib.rs`

- [ ] **Step 1: Write the Expr pyclass**

`rust/monomix-py/src/expr.rs`:

```rust
use crate::errors::CrossSessionError;
use monomix_kernel::{ExprId, ExprNode, ExprPool};
use pyo3::prelude::*;
use std::sync::{Arc, Mutex};

#[pyclass(name = "Expr", module = "monomix._kernel", frozen)]
pub struct Expr {
    pub pool: Arc<Mutex<ExprPool>>,
    pub id: ExprId,
}

impl Expr {
    pub fn new(pool: Arc<Mutex<ExprPool>>, id: ExprId) -> Self {
        Expr { pool, id }
    }

    /// Returns Err with a CrossSessionError if `other` belongs to a different pool.
    pub fn require_same_pool(&self, other: &Expr) -> PyResult<()> {
        if Arc::ptr_eq(&self.pool, &other.pool) {
            Ok(())
        } else {
            Err(PyErr::new::<CrossSessionError, _>(
                "Expr objects come from different Sessions",
            ))
        }
    }
}

#[pymethods]
impl Expr {
    fn __repr__(&self) -> String {
        let pool = self.pool.lock().expect("pool mutex poisoned");
        format!("Expr({:?})", render_node(&pool, self.id))
    }

    fn is_same(&self, other: &Expr) -> bool {
        Arc::ptr_eq(&self.pool, &other.pool) && self.id == other.id
    }

    #[getter]
    fn kind(&self) -> &'static str {
        let pool = self.pool.lock().expect("pool mutex poisoned");
        match pool.get(self.id) {
            ExprNode::SmallInt(_) => "SmallInt",
            ExprNode::BigInt(_) => "BigInt",
            ExprNode::Rational(_) => "Rational",
            ExprNode::Float(_) => "Float",
            ExprNode::Symbol(_) => "Symbol",
            ExprNode::String(_) => "String",
            ExprNode::Add(_) => "Add",
            ExprNode::Mul(_) => "Mul",
            ExprNode::Pow(_, _) => "Pow",
            ExprNode::Neg(_) => "Neg",
            ExprNode::Div(_, _) => "Div",
            ExprNode::Eq(_, _) => "Eq",
            ExprNode::Fn(_, _) => "Fn",
            ExprNode::List(_) => "List",
            ExprNode::Lt(_, _) => "Lt",
            ExprNode::Le(_, _) => "Le",
            ExprNode::Gt(_, _) => "Gt",
            ExprNode::Ge(_, _) => "Ge",
            ExprNode::Not(_) => "Not",
            ExprNode::And(_) => "And",
            ExprNode::Or(_) => "Or",
            ExprNode::Implies(_, _) => "Implies",
            ExprNode::BoolConst(_) => "BoolConst",
        }
    }
}

fn render_node(pool: &ExprPool, id: ExprId) -> String {
    match pool.get(id) {
        ExprNode::SmallInt(n) => n.to_string(),
        ExprNode::BigInt(b) => b.to_string(),
        ExprNode::Rational(r) => format!("{}/{}", r.0, r.1),
        ExprNode::Float(f) => f.into_inner().to_string(),
        ExprNode::Symbol(s) => pool.str_of(*s).to_string(),
        ExprNode::String(s) => format!("\"{}\"", pool.str_of(*s)),
        ExprNode::BoolConst(b) => b.to_string(),
        _ => format!("<{}>", id.0),
    }
}
```

- [ ] **Step 2: Register Expr**

In `rust/monomix-py/src/lib.rs`:

```rust
mod expr;
// ... in the pymodule body:
m.add_class::<expr::Expr>()?;
```

- [ ] **Step 3: Rebuild and confirm import**

```bash
cd python && maturin develop && python -c "from monomix._kernel import Expr; print(Expr.__name__)"
```

Expected: prints `Expr`. (You can't construct one yet — that needs Session.symbol from the next task.)

- [ ] **Step 4: Commit**

```bash
git add rust/monomix-py/src/expr.rs rust/monomix-py/src/lib.rs
git commit -m "Add Expr pyclass with cross-session guard"
```

### Task 3.5: Add constructor methods to `_SessionHandle`

**Files:**
- Modify: `rust/monomix-py/src/session.rs`

- [ ] **Step 1: Add constructors**

Replace the `#[pymethods] impl SessionHandle` block with:

```rust
use crate::errors::map_kernel_error;
use crate::expr::Expr;
use num_bigint::BigInt;

#[pymethods]
impl SessionHandle {
    #[new]
    fn new() -> Self {
        SessionHandle {
            pool: Arc::new(Mutex::new(ExprPool::new())),
        }
    }

    fn symbol(&self, name: &str) -> Expr {
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.symbol(name);
        Expr::new(Arc::clone(&self.pool), id)
    }

    fn integer(&self, n: BigInt) -> Expr {
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.integer(n);
        Expr::new(Arc::clone(&self.pool), id)
    }

    fn rational(&self, p: BigInt, q: BigInt) -> PyResult<Expr> {
        use num_traits::Zero;
        if q.is_zero() {
            return Err(crate::errors::map_kernel_error(
                monomix_kernel::KernelError::DivisionByZero { span: None },
            ));
        }
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.rational(p, q);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn parse(&self, py: Python<'_>, source: &str) -> PyResult<Expr> {
        let pool_arc = Arc::clone(&self.pool);
        let result = py.allow_threads(|| -> Result<ExprId, monomix_kernel::KernelError> {
            let mut pool = pool_arc.lock().expect("pool mutex poisoned");
            let parsed = monomix_kernel::parse(source, &mut pool)?;
            Ok(parsed.root)
        });
        result
            .map(|id| Expr::new(Arc::clone(&self.pool), id))
            .map_err(map_kernel_error)
    }
}
```

Note: `monomix_kernel::parse` signature and return type need to match the kernel's actual API; check `rust/monomix-kernel/src/parser/mod.rs` and adjust the call. The `ParseResult` re-export at the kernel root indicates `parse(src, pool) -> Result<ParseResult, ...>`; `ParseResult` typically has a `root: ExprId` field. If shape differs, follow whatever lib.rs exports.

- [ ] **Step 2: Rebuild**

```bash
cd python && maturin develop
```

Expected: compile success.

- [ ] **Step 3: Smoke test**

```bash
python -c "from monomix._kernel import _SessionHandle; s = _SessionHandle(); x = s.symbol('x'); print(repr(x))"
```

Expected: prints `Expr(x)` (or `Expr("x")` — whatever the render produces).

- [ ] **Step 4: Commit**

```bash
git add rust/monomix-py/src/session.rs
git commit -m "Add atom and parse constructors on _SessionHandle"
```

### Task 3.6: Add the Python `Session` wrapper

**Files:**
- Create: `python/monomix/session.py`
- Create: `python/monomix/__init__.py` (overwrite the placeholder)

- [ ] **Step 1: Write the Session wrapper**

`python/monomix/session.py`:

```python
"""Python-side Session: holds a kernel _SessionHandle plus Python-only state.

The Session owns the ExprPool (indirectly via _SessionHandle). All
mutable state — variable bindings, SMT sort declarations — lives in
the Python class; the kernel itself stays stateless.
"""

from __future__ import annotations

from typing import Literal, Self

from monomix._kernel import Expr, _SessionHandle

Sort = Literal["real", "int", "bool"]


class Session:
    """A monomix evaluation session.

    Owns an ExprPool. Every Expr produced from a Session keeps a
    reference to the underlying pool, so Exprs stay valid past the
    Session's lifetime.
    """

    def __init__(self) -> None:
        self._handle = _SessionHandle()
        # Bindings (`:=`) — Python-side dict; kernel is stateless.
        self._bindings: dict[str, Expr] = {}
        # SMT sort declarations.
        self._sorts: dict[str, Sort] = {}

    # -- atom constructors -------------------------------------------------

    def symbol(self, name: str) -> Expr:
        return self._handle.symbol(name)

    def integer(self, n: int) -> Expr:
        return self._handle.integer(n)

    def rational(self, p: int, q: int) -> Expr:
        return self._handle.rational(p, q)

    def parse(self, source: str) -> Expr:
        return self._handle.parse(source)

    # -- context manager ---------------------------------------------------

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *exc: object) -> None:
        return None
```

- [ ] **Step 2: Wire the public package surface**

Overwrite `python/monomix/__init__.py`:

```python
"""Monomix — modern CAS rewrite of REDUCE."""

from __future__ import annotations

from monomix._kernel import Expr
from monomix.errors import (
    CrossSessionError,
    EvalError,
    MonomixError,
    ParseError,
    UnsupportedError,
)
from monomix.session import Session

__version__ = "0.0.1"

__all__ = [
    "Expr",
    "Session",
    "MonomixError",
    "ParseError",
    "EvalError",
    "UnsupportedError",
    "CrossSessionError",
]
```

- [ ] **Step 3: Smoke test**

```bash
python -c "from monomix import Session; s = Session(); x = s.symbol('x'); print(repr(x), type(x).__module__)"
```

Expected: prints `Expr(x) monomix._kernel`.

- [ ] **Step 4: Commit**

```bash
git add python/monomix/session.py python/monomix/__init__.py
git commit -m "Add monomix.Session wrapper and public package re-exports"
```

### Task 3.7: Delete the placeholder dataclass IR

**Files:**
- Delete: `python/monomix/expr.py`

- [ ] **Step 1: Confirm nothing imports the placeholder**

```bash
grep -r "from monomix.expr" python/ --include="*.py"
grep -r "from monomix import expr" python/ --include="*.py"
```

Expected: matches only inside `python/monomix/solver/` and `python/tests/test_solver.py` — both of which are being replaced in Phase 7.

- [ ] **Step 2: Delete the file**

```bash
git rm python/monomix/expr.py
```

- [ ] **Step 3: Confirm the solver tests still import (they'll be deleted in Phase 7)**

```bash
python -c "import monomix.solver" 2>&1
```

Expected: `ModuleNotFoundError` because `monomix.solver.translate` imports from `..expr`. **Don't fix this yet** — Phase 7 will move and rewrite `solver/`. Leave the solver subpackage broken in the meantime; we'll mark its test file `xfail` next.

- [ ] **Step 4: Skip the solver test file until Phase 7**

Modify `python/tests/test_solver.py` — add at the top:

```python
import pytest
pytest.skip("solver subpackage being rewritten; see phase 7", allow_module_level=True)
```

- [ ] **Step 5: Run tests to confirm they're skipped, not failing**

```bash
cd python && pytest
```

Expected: solver tests skipped; nothing else fails (nothing else exists yet).

- [ ] **Step 6: Commit**

```bash
git add python/monomix/expr.py python/tests/test_solver.py
git commit -m "Drop placeholder dataclass Expr IR; skip solver tests pending rewrite"
```

### Task 3.8: First `.pyi` stub

**Files:**
- Create: `python/monomix/_kernel.pyi`

- [ ] **Step 1: Write the stubs**

```python
"""Type stubs for the monomix._kernel PyO3 extension."""

from __future__ import annotations

from typing import Literal

__version__: str

class MonomixError(Exception): ...
class ParseError(MonomixError): ...
class EvalError(MonomixError): ...
class UnsupportedError(MonomixError): ...
class CrossSessionError(MonomixError): ...

class Expr:
    @property
    def kind(self) -> str: ...
    def is_same(self, other: Expr) -> bool: ...
    def __repr__(self) -> str: ...

class _SessionHandle:
    def __init__(self) -> None: ...
    def symbol(self, name: str) -> Expr: ...
    def integer(self, n: int) -> Expr: ...
    def rational(self, p: int, q: int) -> Expr: ...
    def parse(self, source: str) -> Expr: ...
```

- [ ] **Step 2: Run pyright**

```bash
cd python && pyright --strict monomix
```

Expected: clean (or only reporting issues outside `monomix/_kernel.pyi`).

- [ ] **Step 3: Commit**

```bash
git add python/monomix/_kernel.pyi
git commit -m "Add initial .pyi stubs for monomix._kernel"
```

### Task 3.9: Write Session lifetime test

**Files:**
- Create: `python/tests/__init__.py` (if not present)
- Create: `python/tests/test_session.py`

- [ ] **Step 1: Write the test**

`python/tests/test_session.py`:

```python
from __future__ import annotations

import pytest

from monomix import CrossSessionError, Session


def test_session_yields_expr():
    s = Session()
    x = s.symbol("x")
    assert x.kind == "Symbol"


def test_expr_outlives_session_drop():
    s = Session()
    x = s.symbol("x")
    del s   # Session goes away; Expr should still be valid
    assert x.kind == "Symbol"
    assert repr(x) == "Expr(x)"


def test_expr_is_same_within_session():
    s = Session()
    x1 = s.symbol("x")
    x2 = s.symbol("x")
    assert x1.is_same(x2)


def test_context_manager():
    with Session() as s:
        x = s.symbol("x")
    assert x.kind == "Symbol"


def test_integer_constructor():
    s = Session()
    n = s.integer(42)
    assert n.kind == "SmallInt"


def test_rational_constructor():
    s = Session()
    half = s.rational(1, 2)
    assert half.kind == "Rational"


def test_parse_basic():
    s = Session()
    e = s.parse("x + 1")
    assert e.kind == "Add"
```

- [ ] **Step 2: Run tests**

```bash
cd python && pytest tests/test_session.py -v
```

Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git add python/tests/__init__.py python/tests/test_session.py
git commit -m "Add Session/Expr lifetime tests"
```

---

## Phase 4: Operator overloading

Add arithmetic, comparison, and boolean operator support on `Expr`. `__bool__` and `__hash__` rules locked in.

### Task 4.1: Arithmetic operators

**Files:**
- Modify: `rust/monomix-py/src/expr.rs`

- [ ] **Step 1: Write the failing test**

Add to `python/tests/test_expr.py` (create the file):

```python
from __future__ import annotations

import pytest

from monomix import Session


@pytest.fixture
def s():
    return Session()


@pytest.fixture
def x(s):
    return s.symbol("x")


@pytest.fixture
def y(s):
    return s.symbol("y")


def test_add(x, y):
    assert (x + y).kind == "Add"


def test_sub(x, y):
    e = x - y
    assert e.kind == "Add"  # x + (-y), flattens
    # children should include x and Neg(y)


def test_mul(x, y):
    assert (x * y).kind == "Mul"


def test_div(x, y):
    assert (x / y).kind == "Div"


def test_pow(x, s):
    assert (x ** s.integer(2)).kind == "Pow"


def test_neg(x):
    assert (-x).kind == "Neg"


def test_literal_coercion_add(x):
    e = x + 1
    assert e.kind == "Add"


def test_literal_coercion_radd(x):
    e = 1 + x
    assert e.kind == "Add"


def test_literal_coercion_mul(x):
    assert (2 * x).kind == "Mul"
```

- [ ] **Step 2: Run to verify failure**

```bash
pytest python/tests/test_expr.py -v
```

Expected: FAIL — `unsupported operand type(s)`.

- [ ] **Step 3: Implement the operators**

Add to `rust/monomix-py/src/expr.rs` inside `#[pymethods] impl Expr`:

```rust
use num_bigint::BigInt;
use pyo3::types::PyAny;

fn coerce_to_expr(py: Python<'_>, value: &Bound<'_, PyAny>, pool: &Arc<Mutex<ExprPool>>) -> PyResult<Expr> {
    if let Ok(e) = value.extract::<PyRef<Expr>>() {
        if !Arc::ptr_eq(&e.pool, pool) {
            return Err(PyErr::new::<CrossSessionError, _>(
                "Expr objects come from different Sessions",
            ));
        }
        return Ok(Expr::new(Arc::clone(pool), e.id));
    }
    if let Ok(n) = value.extract::<BigInt>() {
        let mut p = pool.lock().expect("pool mutex poisoned");
        let id = p.integer(n);
        return Ok(Expr::new(Arc::clone(pool), id));
    }
    if let Ok(f) = value.extract::<f64>() {
        let mut p = pool.lock().expect("pool mutex poisoned");
        let id = p.float(f);
        return Ok(Expr::new(Arc::clone(pool), id));
    }
    Err(pyo3::exceptions::PyTypeError::new_err(
        "operand must be Expr, int, or float",
    ))
}

#[pymethods]
impl Expr {
    // ... existing methods ...

    fn __add__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let rhs = coerce_to_expr(py, other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.add(vec![self.id, rhs.id]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __radd__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let lhs = coerce_to_expr(py, other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.add(vec![lhs.id, self.id]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __sub__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let rhs = coerce_to_expr(py, other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let neg_b = pool.neg(rhs.id);
        let id = pool.add(vec![self.id, neg_b]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __rsub__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let lhs = coerce_to_expr(py, other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let neg_self = pool.neg(self.id);
        let id = pool.add(vec![lhs.id, neg_self]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __mul__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let rhs = coerce_to_expr(py, other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.mul(vec![self.id, rhs.id]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __rmul__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let lhs = coerce_to_expr(py, other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.mul(vec![lhs.id, self.id]);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __truediv__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let rhs = coerce_to_expr(py, other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.div(self.id, rhs.id);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __rtruediv__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
        let lhs = coerce_to_expr(py, other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.div(lhs.id, self.id);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __pow__(&self, py: Python<'_>, other: &Bound<'_, PyAny>, _mod: Option<&Bound<'_, PyAny>>) -> PyResult<Expr> {
        let rhs = coerce_to_expr(py, other, &self.pool)?;
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.pow(self.id, rhs.id);
        Ok(Expr::new(Arc::clone(&self.pool), id))
    }

    fn __neg__(&self) -> Expr {
        let mut pool = self.pool.lock().expect("pool mutex poisoned");
        let id = pool.neg(self.id);
        Expr::new(Arc::clone(&self.pool), id)
    }
}
```

- [ ] **Step 4: Rebuild and run tests**

```bash
cd python && maturin develop && pytest tests/test_expr.py -v
```

Expected: arithmetic tests pass.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-py/src/expr.rs python/tests/test_expr.py
git commit -m "Overload arithmetic operators on Expr"
```

### Task 4.2: Equality, hashing, and `__bool__`

**Files:**
- Modify: `rust/monomix-py/src/expr.rs`

- [ ] **Step 1: Write failing tests**

Append to `python/tests/test_expr.py`:

```python
def test_eq_builds_eq_node(x, y):
    e = x == y
    assert e.kind == "Eq"


def test_eq_self_is_true(x):
    # Eq(x, x) → __bool__ True via handle equality
    assert bool(x == x) is True


def test_eq_different_symbols_is_false(x, y):
    assert bool(x == y) is False


def test_ne_builds_not_eq(x, y):
    e = x != y
    assert e.kind == "Not"


def test_bool_of_non_eq_raises(x, y):
    e = x + y
    with pytest.raises(TypeError):
        bool(e)


def test_hash_consistency(x, s):
    x2 = s.symbol("x")
    assert hash(x) == hash(x2)
    assert bool(x == x2)


def test_hash_differs_for_distinct(x, y):
    assert hash(x) != hash(y)


def test_dict_key(x, s):
    x2 = s.symbol("x")
    d = {x: "value"}
    assert d[x2] == "value"


def test_eq_with_int_literal(x):
    e = x == 0
    assert e.kind == "Eq"
```

- [ ] **Step 2: Run to verify failure**

```bash
pytest python/tests/test_expr.py -v
```

Expected: `__eq__` either returns Python's default (identity) or unimplemented.

- [ ] **Step 3: Implement `__eq__`, `__ne__`, `__bool__`, `__hash__`**

Add to `#[pymethods] impl Expr`:

```rust
fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
    let rhs = coerce_to_expr(py, other, &self.pool)?;
    let mut pool = self.pool.lock().expect("pool mutex poisoned");
    let id = pool.eq_node(self.id, rhs.id);
    Ok(Expr::new(Arc::clone(&self.pool), id))
}

fn __ne__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
    let rhs = coerce_to_expr(py, other, &self.pool)?;
    let mut pool = self.pool.lock().expect("pool mutex poisoned");
    let eq = pool.eq_node(self.id, rhs.id);
    let id = pool.not_node(eq);
    Ok(Expr::new(Arc::clone(&self.pool), id))
}

fn __bool__(&self) -> PyResult<bool> {
    let pool = self.pool.lock().expect("pool mutex poisoned");
    match pool.get(self.id) {
        ExprNode::Eq(a, b) => Ok(a == b),
        ExprNode::Not(inner) => {
            // Only Not(Eq(a, b)) is bool-evaluable; everything else errors.
            match pool.get(*inner) {
                ExprNode::Eq(a, b) => Ok(a != b),
                _ => Err(pyo3::exceptions::PyTypeError::new_err(
                    "ambiguous truth value of symbolic expression — use is_same() or evaluate first",
                )),
            }
        }
        ExprNode::BoolConst(b) => Ok(*b),
        _ => Err(pyo3::exceptions::PyTypeError::new_err(
            "ambiguous truth value of symbolic expression — use is_same() or evaluate first",
        )),
    }
}

fn __hash__(&self) -> u64 {
    self.id.0 as u64
}
```

- [ ] **Step 4: Rebuild and run tests**

```bash
cd python && maturin develop && pytest tests/test_expr.py -v
```

Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-py/src/expr.rs python/tests/test_expr.py
git commit -m "Overload __eq__, __ne__, __bool__, __hash__ on Expr"
```

### Task 4.3: Comparison operators (`<`, `<=`, `>`, `>=`)

**Files:**
- Modify: `rust/monomix-py/src/expr.rs`

- [ ] **Step 1: Write failing tests**

Append to `python/tests/test_expr.py`:

```python
def test_lt(x, y):
    assert (x < y).kind == "Lt"


def test_le(x, y):
    assert (x <= y).kind == "Le"


def test_gt(x, y):
    assert (x > y).kind == "Gt"


def test_ge(x, y):
    assert (x >= y).kind == "Ge"


def test_lt_bool_raises(x, y):
    with pytest.raises(TypeError):
        bool(x < y)
```

- [ ] **Step 2: Run to verify failure**

```bash
pytest python/tests/test_expr.py::test_lt -v
```

Expected: FAIL.

- [ ] **Step 3: Implement comparison ops**

Add `__lt__`, `__le__`, `__gt__`, `__ge__` to `#[pymethods] impl Expr`:

```rust
fn __lt__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
    let rhs = coerce_to_expr(py, other, &self.pool)?;
    let mut pool = self.pool.lock().expect("pool mutex poisoned");
    let id = pool.lt(self.id, rhs.id);
    Ok(Expr::new(Arc::clone(&self.pool), id))
}

fn __le__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
    let rhs = coerce_to_expr(py, other, &self.pool)?;
    let mut pool = self.pool.lock().expect("pool mutex poisoned");
    let id = pool.le(self.id, rhs.id);
    Ok(Expr::new(Arc::clone(&self.pool), id))
}

fn __gt__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
    let rhs = coerce_to_expr(py, other, &self.pool)?;
    let mut pool = self.pool.lock().expect("pool mutex poisoned");
    let id = pool.gt(self.id, rhs.id);
    Ok(Expr::new(Arc::clone(&self.pool), id))
}

fn __ge__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
    let rhs = coerce_to_expr(py, other, &self.pool)?;
    let mut pool = self.pool.lock().expect("pool mutex poisoned");
    let id = pool.ge(self.id, rhs.id);
    Ok(Expr::new(Arc::clone(&self.pool), id))
}
```

- [ ] **Step 4: Rebuild and test**

```bash
cd python && maturin develop && pytest tests/test_expr.py -v
```

Expected: green.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-py/src/expr.rs python/tests/test_expr.py
git commit -m "Overload <, <=, >, >= on Expr"
```

### Task 4.4: Boolean operators (`&`, `|`, `~`)

**Files:**
- Modify: `rust/monomix-py/src/expr.rs`

- [ ] **Step 1: Write failing tests**

Append:

```python
def test_and(x, y):
    a = (x == 0)
    b = (y == 0)
    e = a & b
    assert e.kind == "And"


def test_or(x, y):
    a = (x == 0)
    b = (y == 0)
    e = a | b
    assert e.kind == "Or"


def test_invert_eq(x):
    a = (x == 0)
    e = ~a
    assert e.kind == "Not"
```

- [ ] **Step 2: Run to verify failure**

```bash
pytest python/tests/test_expr.py -v -k "test_and or test_or or test_invert"
```

Expected: FAIL.

- [ ] **Step 3: Implement `__and__`, `__or__`, `__invert__`**

Add:

```rust
fn __and__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
    let rhs = coerce_to_expr(py, other, &self.pool)?;
    let mut pool = self.pool.lock().expect("pool mutex poisoned");
    let id = pool.and_(vec![self.id, rhs.id]);
    Ok(Expr::new(Arc::clone(&self.pool), id))
}

fn __rand__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
    let lhs = coerce_to_expr(py, other, &self.pool)?;
    let mut pool = self.pool.lock().expect("pool mutex poisoned");
    let id = pool.and_(vec![lhs.id, self.id]);
    Ok(Expr::new(Arc::clone(&self.pool), id))
}

fn __or__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
    let rhs = coerce_to_expr(py, other, &self.pool)?;
    let mut pool = self.pool.lock().expect("pool mutex poisoned");
    let id = pool.or_(vec![self.id, rhs.id]);
    Ok(Expr::new(Arc::clone(&self.pool), id))
}

fn __ror__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Expr> {
    let lhs = coerce_to_expr(py, other, &self.pool)?;
    let mut pool = self.pool.lock().expect("pool mutex poisoned");
    let id = pool.or_(vec![lhs.id, self.id]);
    Ok(Expr::new(Arc::clone(&self.pool), id))
}

fn __invert__(&self) -> Expr {
    let mut pool = self.pool.lock().expect("pool mutex poisoned");
    let id = pool.not_node(self.id);
    Expr::new(Arc::clone(&self.pool), id)
}
```

- [ ] **Step 4: Rebuild and test**

```bash
cd python && maturin develop && pytest tests/test_expr.py -v
```

Expected: green.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-py/src/expr.rs python/tests/test_expr.py
git commit -m "Overload &, |, ~ on Expr"
```

### Task 4.5: Cross-session guard tests

**Files:**
- Modify: `python/tests/test_expr.py`

- [ ] **Step 1: Write failing test**

```python
def test_cross_session_add_raises():
    from monomix import CrossSessionError
    s1 = Session()
    s2 = Session()
    x = s1.symbol("x")
    y = s2.symbol("y")
    with pytest.raises(CrossSessionError):
        _ = x + y


def test_cross_session_eq_raises():
    from monomix import CrossSessionError
    s1 = Session()
    s2 = Session()
    x = s1.symbol("x")
    y = s2.symbol("y")
    with pytest.raises(CrossSessionError):
        _ = (x == y)
```

- [ ] **Step 2: Run to verify**

```bash
pytest python/tests/test_expr.py -v -k "cross_session"
```

Expected: pass — the `coerce_to_expr` helper already raises `CrossSessionError` when pools differ.

- [ ] **Step 3: Commit**

```bash
git add python/tests/test_expr.py
git commit -m "Verify cross-session operator guards"
```

### Task 4.6: Update `.pyi` for operators

**Files:**
- Modify: `python/monomix/_kernel.pyi`

- [ ] **Step 1: Extend the Expr stub**

Replace the `Expr` class block in `python/monomix/_kernel.pyi`:

```python
class Expr:
    @property
    def kind(self) -> str: ...
    def is_same(self, other: Expr) -> bool: ...
    def __repr__(self) -> str: ...
    def __hash__(self) -> int: ...
    def __bool__(self) -> bool: ...

    # Arithmetic
    def __add__(self, other: Expr | int | float) -> Expr: ...
    def __radd__(self, other: Expr | int | float) -> Expr: ...
    def __sub__(self, other: Expr | int | float) -> Expr: ...
    def __rsub__(self, other: Expr | int | float) -> Expr: ...
    def __mul__(self, other: Expr | int | float) -> Expr: ...
    def __rmul__(self, other: Expr | int | float) -> Expr: ...
    def __truediv__(self, other: Expr | int | float) -> Expr: ...
    def __rtruediv__(self, other: Expr | int | float) -> Expr: ...
    def __pow__(self, other: Expr | int | float) -> Expr: ...
    def __neg__(self) -> Expr: ...

    # Comparison / equality
    def __eq__(self, other: object) -> Expr: ...  # type: ignore[override]
    def __ne__(self, other: object) -> Expr: ...  # type: ignore[override]
    def __lt__(self, other: Expr | int | float) -> Expr: ...
    def __le__(self, other: Expr | int | float) -> Expr: ...
    def __gt__(self, other: Expr | int | float) -> Expr: ...
    def __ge__(self, other: Expr | int | float) -> Expr: ...

    # Boolean
    def __and__(self, other: Expr) -> Expr: ...
    def __rand__(self, other: Expr) -> Expr: ...
    def __or__(self, other: Expr) -> Expr: ...
    def __ror__(self, other: Expr) -> Expr: ...
    def __invert__(self) -> Expr: ...
```

- [ ] **Step 2: Run pyright**

```bash
cd python && pyright --strict monomix tests
```

Expected: clean (or near-clean — minor warnings are acceptable for v1).

- [ ] **Step 3: Commit**

```bash
git add python/monomix/_kernel.pyi
git commit -m "Add Expr operator overloads to .pyi stubs"
```

---

## Phase 5: Module-level kernel functions

Add `parse`, `simplify`, `df`, `expand`, `solve`, `sub`, `evaluate_numeric` at module level. GIL release on each. Error mapping through.

### Task 5.1: `simplify`

**Files:**
- Create: `rust/monomix-py/src/kernel_fns.rs`
- Modify: `rust/monomix-py/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `python/tests/test_kernel_calls.py`:

```python
from __future__ import annotations

import pytest

from monomix import Session, simplify


def test_simplify_constant_folds():
    s = Session()
    e = s.parse("0 + x")
    result = simplify(e)
    assert result.is_same(s.symbol("x"))
```

- [ ] **Step 2: Run to verify failure**

```bash
pytest python/tests/test_kernel_calls.py -v
```

Expected: `ImportError: cannot import name 'simplify'`.

- [ ] **Step 3: Implement `simplify`**

Create `rust/monomix-py/src/kernel_fns.rs`:

```rust
use crate::errors::map_kernel_error;
use crate::expr::Expr;
use monomix_kernel::ExprId;
use pyo3::prelude::*;
use std::sync::Arc;

#[pyfunction]
pub fn simplify(py: Python<'_>, e: &Expr) -> PyResult<Expr> {
    let pool_arc = Arc::clone(&e.pool);
    let original_id = e.id;
    let new_id = py.allow_threads(|| -> Result<ExprId, monomix_kernel::KernelError> {
        let mut pool = pool_arc.lock().expect("pool mutex poisoned");
        Ok(monomix_kernel::simplify::simplify(&mut pool, original_id))
    });
    new_id
        .map(|id| Expr::new(Arc::clone(&e.pool), id))
        .map_err(map_kernel_error)
}
```

(Verify the actual path of `simplify` in the kernel — look at `rust/monomix-kernel/src/lib.rs` and adjust the use path. If `simplify::simplify` returns the new `ExprId` directly (not `Result`), drop the `Result` wrapping.)

- [ ] **Step 4: Register in lib.rs**

In `rust/monomix-py/src/lib.rs`:

```rust
mod kernel_fns;
// in pymodule body:
m.add_function(wrap_pyfunction!(kernel_fns::simplify, m)?)?;
```

Add `use pyo3::wrap_pyfunction;` at the top.

- [ ] **Step 5: Re-export from Python**

In `python/monomix/__init__.py`:

```python
from monomix._kernel import Expr, simplify  # add simplify
```

And to `__all__`.

Also update `_kernel.pyi`:

```python
def simplify(e: Expr) -> Expr: ...
```

- [ ] **Step 6: Rebuild and test**

```bash
cd python && maturin develop && pytest tests/test_kernel_calls.py -v
```

Expected: green.

- [ ] **Step 7: Commit**

```bash
git add rust/monomix-py/src/kernel_fns.rs rust/monomix-py/src/lib.rs python/monomix/__init__.py python/monomix/_kernel.pyi python/tests/test_kernel_calls.py
git commit -m "Add monomix.simplify module-level function"
```

### Task 5.2: `df` (differentiate)

**Files:**
- Modify: `rust/monomix-py/src/kernel_fns.rs`, `lib.rs`, `__init__.py`, `_kernel.pyi`

- [ ] **Step 1: Write failing test**

Append to `python/tests/test_kernel_calls.py`:

```python
from monomix import df


def test_df_polynomial():
    s = Session()
    x = s.symbol("x")
    expr = x ** s.integer(3)
    d = df(expr, x)
    # Without simplification, d may be unevaluated chain product;
    # after simplify, expect 3 * x^2.
    from monomix import simplify
    result = simplify(d)
    expected = s.integer(3) * (x ** s.integer(2))
    assert simplify(result).is_same(simplify(expected))


def test_df_unsupported_on_comparison():
    from monomix import UnsupportedError
    s = Session()
    x = s.symbol("x")
    y = s.symbol("y")
    with pytest.raises(UnsupportedError):
        df(x < y, x)
```

- [ ] **Step 2: Implement `df`**

Add to `rust/monomix-py/src/kernel_fns.rs`:

```rust
#[pyfunction]
pub fn df(py: Python<'_>, e: &Expr, x: &Expr) -> PyResult<Expr> {
    if !std::sync::Arc::ptr_eq(&e.pool, &x.pool) {
        return Err(PyErr::new::<crate::errors::CrossSessionError, _>(
            "df: Expr and variable come from different Sessions",
        ));
    }
    let pool_arc = Arc::clone(&e.pool);
    let e_id = e.id;
    let x_id = x.id;
    let new_id = py.allow_threads(|| -> Result<ExprId, monomix_kernel::KernelError> {
        let mut pool = pool_arc.lock().expect("pool mutex poisoned");
        monomix_kernel::differentiate(&mut pool, e_id, x_id)
    });
    new_id
        .map(|id| Expr::new(Arc::clone(&e.pool), id))
        .map_err(map_kernel_error)
}
```

Register, re-export, update `.pyi`, rebuild, run tests.

- [ ] **Step 3: Rebuild and test**

```bash
cd python && maturin develop && pytest tests/test_kernel_calls.py -v
```

Expected: green.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "Add monomix.df module-level function"
```

### Task 5.3: `expand`, `solve`, `sub`, `evaluate_numeric`

Each follows the same pattern as `simplify`/`df`. Write the test, add the `#[pyfunction]`, register, re-export, update `.pyi`, run tests, commit.

**Files:**
- Modify: same as task 5.1

- [ ] **Step 1: Write `expand` test and implementation**

```python
# In test_kernel_calls.py
from monomix import expand


def test_expand_product():
    s = Session()
    x = s.symbol("x")
    expr = (x + s.integer(1)) * (x + s.integer(1))
    result = expand(expr)
    # x^2 + 2x + 1
    # Just verify it expanded into an Add of three terms
    assert result.kind == "Add"
```

```rust
#[pyfunction]
pub fn expand(py: Python<'_>, e: &Expr) -> PyResult<Expr> {
    let pool_arc = Arc::clone(&e.pool);
    let e_id = e.id;
    let new_id = py.allow_threads(|| -> Result<ExprId, monomix_kernel::KernelError> {
        let mut pool = pool_arc.lock().expect("pool mutex poisoned");
        Ok(monomix_kernel::poly::expand(&mut pool, e_id))
    });
    new_id
        .map(|id| Expr::new(Arc::clone(&e.pool), id))
        .map_err(map_kernel_error)
}
```

(Verify the actual kernel API in `rust/monomix-kernel/src/poly/mod.rs` — adjust the path.)

- [ ] **Step 2: Write `solve` test and implementation**

```python
from monomix import solve


def test_solve_linear():
    s = Session()
    x = s.symbol("x")
    eq = (x * s.integer(2) - s.integer(4)) == s.integer(0)
    solutions = solve(eq, x)
    assert len(solutions) >= 1
```

```rust
#[pyfunction]
pub fn solve(py: Python<'_>, eq: &Expr, x: &Expr) -> PyResult<Vec<Expr>> {
    if !std::sync::Arc::ptr_eq(&eq.pool, &x.pool) {
        return Err(PyErr::new::<crate::errors::CrossSessionError, _>(
            "solve: arguments from different Sessions",
        ));
    }
    let pool_arc = Arc::clone(&eq.pool);
    let eq_id = eq.id;
    let x_id = x.id;
    let ids = py.allow_threads(|| -> Result<Vec<ExprId>, monomix_kernel::KernelError> {
        let mut pool = pool_arc.lock().expect("pool mutex poisoned");
        monomix_kernel::solve::solve(&mut pool, eq_id, x_id)
    });
    ids.map(|v| {
        v.into_iter()
            .map(|id| Expr::new(Arc::clone(&eq.pool), id))
            .collect()
    })
    .map_err(map_kernel_error)
}
```

- [ ] **Step 3: Write `sub` test and implementation**

```python
from monomix import sub


def test_sub_replaces_symbol():
    s = Session()
    x = s.symbol("x")
    expr = x + s.integer(1)
    result = sub({x: s.integer(5)}, expr)
    from monomix import simplify
    assert simplify(result).is_same(s.integer(6))
```

```rust
#[pyfunction]
pub fn sub(py: Python<'_>, mapping: &Bound<'_, pyo3::types::PyDict>, e: &Expr) -> PyResult<Expr> {
    let pool_arc = Arc::clone(&e.pool);
    let mut pairs: Vec<(ExprId, ExprId)> = Vec::with_capacity(mapping.len());
    for (k, v) in mapping.iter() {
        let k_expr: PyRef<Expr> = k.extract()?;
        let v_expr: PyRef<Expr> = v.extract()?;
        if !Arc::ptr_eq(&k_expr.pool, &pool_arc) || !Arc::ptr_eq(&v_expr.pool, &pool_arc) {
            return Err(PyErr::new::<crate::errors::CrossSessionError, _>(
                "sub: mapping contains Expr from a different Session",
            ));
        }
        pairs.push((k_expr.id, v_expr.id));
    }
    let e_id = e.id;
    let new_id = py.allow_threads(|| -> Result<ExprId, monomix_kernel::KernelError> {
        let mut pool = pool_arc.lock().expect("pool mutex poisoned");
        monomix_kernel::substitute::substitute_multi(&mut pool, e_id, &pairs)
    });
    new_id
        .map(|id| Expr::new(Arc::clone(&e.pool), id))
        .map_err(map_kernel_error)
}
```

(Verify the actual kernel API — `monomix_kernel::substitute::substitute_multi` may have a different name/signature.)

- [ ] **Step 4: Write `evaluate_numeric` test and implementation**

```python
from monomix import evaluate_numeric


def test_evaluate_numeric_constant():
    s = Session()
    e = s.integer(3) + s.integer(4)
    assert evaluate_numeric(e) == pytest.approx(7.0)


def test_evaluate_numeric_unbound_symbol_raises():
    from monomix import EvalError
    s = Session()
    x = s.symbol("x")
    with pytest.raises(EvalError):
        evaluate_numeric(x)
```

```rust
#[pyfunction]
pub fn evaluate_numeric(py: Python<'_>, e: &Expr) -> PyResult<f64> {
    let pool_arc = Arc::clone(&e.pool);
    let e_id = e.id;
    let result = py.allow_threads(|| -> Result<f64, monomix_kernel::KernelError> {
        let pool = pool_arc.lock().expect("pool mutex poisoned");
        monomix_kernel::evalnum::evaluate_numeric(&pool, e_id, &Default::default())
    });
    result.map_err(map_kernel_error)
}
```

(Verify the actual evalnum API in `rust/monomix-kernel/src/evalnum/mod.rs`.)

- [ ] **Step 5: Register, re-export, update `.pyi`, run tests**

For each new function: register in `lib.rs`, add to `__init__.py` and `__all__`, update `.pyi` stubs.

`_kernel.pyi`:

```python
def simplify(e: Expr) -> Expr: ...
def df(e: Expr, x: Expr) -> Expr: ...
def expand(e: Expr) -> Expr: ...
def solve(eq: Expr, x: Expr) -> list[Expr]: ...
def sub(mapping: dict[Expr, Expr], e: Expr) -> Expr: ...
def evaluate_numeric(e: Expr) -> float: ...
```

- [ ] **Step 6: Rebuild and run all tests**

```bash
cd python && maturin develop && pytest -v
```

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "Add monomix.expand, solve, sub, evaluate_numeric module-level functions"
```

### Task 5.4: GIL release verification

**Files:**
- Create: `python/tests/test_gil.py`

- [ ] **Step 1: Write the soft-floor parallelism test**

```python
"""Verifies that simplify releases the GIL: two simplifies on two
Sessions, run concurrently from two Python threads, should not take
2× as long as a single one.

This is a SOFT FLOOR: the assertion uses a generous tolerance because
CI machines vary. Flag for follow-up if it false-fails consistently."""

from __future__ import annotations

import threading
import time

import pytest

from monomix import Session, simplify


def _heavy_expr(s: Session):
    x = s.symbol("x")
    expr = x
    for i in range(1, 30):
        expr = expr + (x ** s.integer(i))
    return expr


@pytest.mark.benchmark
def test_simplify_releases_gil():
    s1, s2 = Session(), Session()
    e1, e2 = _heavy_expr(s1), _heavy_expr(s2)

    # Serial baseline
    t0 = time.perf_counter()
    _ = simplify(e1)
    _ = simplify(e2)
    serial = time.perf_counter() - t0

    # Concurrent
    def worker(e):
        _ = simplify(e)

    t1 = time.perf_counter()
    threads = [threading.Thread(target=worker, args=(e1,)),
               threading.Thread(target=worker, args=(e2,))]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    parallel = time.perf_counter() - t1

    # Soft floor: parallel should be substantially less than 2× serial
    # if the GIL is actually released. Tolerate up to 1.7× serial/2,
    # i.e., parallel < 0.85 * serial in best case, but allow up to
    # serial * 0.95 for noisy CI. Anything close to serial * 1.0
    # means concurrency is fully serialized.
    assert parallel < serial, f"parallel ({parallel:.3f}s) not faster than serial ({serial:.3f}s)"
```

- [ ] **Step 2: Run the test**

```bash
pytest python/tests/test_gil.py -v
```

Expected: pass on a machine with >1 CPU. On a 1-CPU CI runner, this will false-fail; in that case mark as `@pytest.mark.skip_if_single_cpu` and document.

- [ ] **Step 3: Commit**

```bash
git add python/tests/test_gil.py
git commit -m "Add soft-floor GIL-release parallelism test"
```

---

## Phase 6: Session bindings + sort declarations

Add `assign`, `clear`, `bindings`, `declare`. Wire `parse` to resolve bindings.

### Task 6.1: Add `Session.declare`

**Files:**
- Modify: `python/monomix/session.py`, `python/tests/test_session.py`

- [ ] **Step 1: Write failing test**

Append to `python/tests/test_session.py`:

```python
def test_declare_sort():
    s = Session()
    s.declare("n", "int")
    assert s.sort_of("n") == "int"


def test_declare_default_real():
    s = Session()
    assert s.sort_of("x") == "real"


def test_declare_invalid_sort_raises():
    s = Session()
    with pytest.raises(ValueError):
        s.declare("x", "complex")  # not a Sort literal


def test_declare_with_explicit_real():
    s = Session()
    s.declare("y", "real")
    assert s.sort_of("y") == "real"
```

- [ ] **Step 2: Run to verify failure**

```bash
pytest python/tests/test_session.py -v -k declare
```

Expected: FAIL — `declare` doesn't exist.

- [ ] **Step 3: Implement**

In `python/monomix/session.py`, add methods:

```python
    # -- SMT sort declarations --------------------------------------------

    def declare(self, name: str, sort: Sort) -> None:
        if sort not in ("real", "int", "bool"):
            raise ValueError(f"sort must be 'real', 'int', or 'bool'; got {sort!r}")
        self._sorts[name] = sort

    def sort_of(self, name: str) -> Sort:
        return self._sorts.get(name, "real")
```

- [ ] **Step 4: Run tests**

```bash
pytest python/tests/test_session.py -v
```

Expected: green.

- [ ] **Step 5: Commit**

```bash
git add python/monomix/session.py python/tests/test_session.py
git commit -m "Add Session.declare and sort_of for SMT sort metadata"
```

### Task 6.2: Add bindings (`assign`, `clear`, `bindings`)

**Files:**
- Modify: `python/monomix/session.py`, `python/tests/test_session.py`

- [ ] **Step 1: Write failing tests**

```python
def test_assign_and_clear():
    s = Session()
    x = s.symbol("x")
    s.assign("a", x)
    assert "a" in s.bindings()
    s.clear("a")
    assert "a" not in s.bindings()


def test_clear_missing_is_noop():
    s = Session()
    s.clear("nope")  # should not raise
```

- [ ] **Step 2: Implement**

```python
    # -- Bindings ----------------------------------------------------------

    def assign(self, name: str, value: Expr) -> None:
        self._bindings[name] = value

    def clear(self, name: str) -> None:
        self._bindings.pop(name, None)

    def bindings(self) -> dict[str, Expr]:
        return dict(self._bindings)  # defensive copy
```

- [ ] **Step 3: Run tests**

```bash
pytest python/tests/test_session.py -v
```

Expected: green.

- [ ] **Step 4: Commit**

```bash
git add python/monomix/session.py python/tests/test_session.py
git commit -m "Add Session bindings: assign, clear, bindings"
```

### Task 6.3: Parse resolves bindings

**Files:**
- Modify: `python/monomix/session.py`, `python/tests/test_session.py`

- [ ] **Step 1: Write failing test**

```python
def test_parse_resolves_bindings():
    from monomix import simplify
    s = Session()
    x = s.symbol("x")
    s.assign("a", x + s.integer(1))
    result = s.parse("a + 1")
    # After substitution: (x + 1) + 1 = x + 2 after simplify
    expected = x + s.integer(2)
    assert simplify(result).is_same(simplify(expected))
```

- [ ] **Step 2: Implement binding resolution in `Session.parse`**

Update `Session.parse` to substitute bindings before returning:

```python
    def parse(self, source: str) -> Expr:
        parsed = self._handle.parse(source)
        if not self._bindings:
            return parsed
        # Build {name_symbol: value_expr} mapping
        from monomix._kernel import sub
        mapping = {self.symbol(name): value for name, value in self._bindings.items()}
        return sub(mapping, parsed)
```

(`sub` is the module-level function from Phase 5.)

- [ ] **Step 3: Run test**

```bash
pytest python/tests/test_session.py -v
```

Expected: green.

- [ ] **Step 4: Commit**

```bash
git add python/monomix/session.py python/tests/test_session.py
git commit -m "Resolve Session bindings in parse"
```

---

## Phase 7: SMT bridge rewrite

Move `python/monomix/solver/` → `python/monomix/smt/`. Rewrite the translator to consume the new Rust-backed `Expr`. Port + extend tests.

### Task 7.1: Move directory and rename package

**Files:**
- Move: `python/monomix/solver/` → `python/monomix/smt/`

- [ ] **Step 1: Move the directory**

```bash
git mv python/monomix/solver python/monomix/smt
```

- [ ] **Step 2: Update internal imports inside the moved package**

Replace `..expr` imports (they pointed at the deleted dataclass IR). Files that need touching:
- `python/monomix/smt/__init__.py`
- `python/monomix/smt/translate.py`
- `python/monomix/smt/z3_backend.py`

Apply `from ..expr import ...` → marked as TODO; the next tasks rewrite the consumers.

- [ ] **Step 3: Update docstrings to drop "Z3 bridge" phrasing**

In `python/monomix/smt/__init__.py`, change docstring to say "SMT bridge" instead of "Solver facade". Z3-specific phrasing only stays inside `z3_backend.py`.

- [ ] **Step 4: Commit (intermediate; will be broken until next task)**

```bash
git add python/monomix/smt/ python/monomix/solver/
git commit -m "Move monomix.solver -> monomix.smt (still imports old IR; next task rewrites)"
```

### Task 7.2: Rewrite the translator

**Files:**
- Rewrite: `python/monomix/smt/translate.py`

- [ ] **Step 1: Replace the file's contents**

```python
"""Translate Monomix Expr (Rust-backed) into backend AST.

The translator walks a kernel ExprNode tree via the Expr inspection
API (`expr.kind`, `expr.children()`, `expr.as_int()`, etc.) and emits
backend-specific terms via a small Backend protocol.

Z3 is the current backend; the protocol is the integration point for
any future backend.
"""

from __future__ import annotations

from fractions import Fraction
from typing import Any, Protocol

from monomix import Expr, Session

from .errors import TranslationError, Unsupported


class Backend(Protocol):
    """Minimum interface a backend must provide."""

    def real(self, name: str) -> Any: ...
    def int(self, name: str) -> Any: ...
    def bool(self, name: str) -> Any: ...
    def rational_const(self, num: int, den: int) -> Any: ...
    def int_const(self, n: int) -> Any: ...
    def bool_const(self, b: bool) -> Any: ...

    def add(self, *xs: Any) -> Any: ...
    def mul(self, *xs: Any) -> Any: ...
    def neg(self, x: Any) -> Any: ...
    def div(self, a: Any, b: Any) -> Any: ...
    def pow_int(self, base: Any, n: int) -> Any: ...

    def eq(self, a: Any, b: Any) -> Any: ...
    def lt(self, a: Any, b: Any) -> Any: ...
    def le(self, a: Any, b: Any) -> Any: ...
    def gt(self, a: Any, b: Any) -> Any: ...
    def ge(self, a: Any, b: Any) -> Any: ...

    def and_(self, *xs: Any) -> Any: ...
    def or_(self, *xs: Any) -> Any: ...
    def not_(self, x: Any) -> Any: ...
    def implies(self, a: Any, b: Any) -> Any: ...

    def uninterpreted(self, name: str, args: list[Any]) -> Any: ...


class Translator:
    """Stateful translator caching backend declarations per symbol."""

    def __init__(self, backend: Backend, session: Session) -> None:
        self.backend = backend
        self.session = session
        self._symbols: dict[tuple[str, str], Any] = {}

    def to_backend(self, e: Expr) -> Any:
        kind = e.kind

        # Atoms
        if kind == "SmallInt":
            n = e.as_int()
            assert n is not None
            return self.backend.int_const(n)
        if kind == "BigInt":
            n = e.as_int()
            assert n is not None
            return self.backend.int_const(n)
        if kind == "Rational":
            r = e.as_rational()
            assert r is not None
            return self.backend.rational_const(r[0], r[1])
        if kind == "Float":
            f = e.as_float()
            assert f is not None
            # Floats round to rationals for backend exactness — approximate.
            frac = Fraction(f).limit_denominator(10**12)
            return self.backend.rational_const(frac.numerator, frac.denominator)
        if kind == "Symbol":
            name = e.symbol_name()
            assert name is not None
            return self._declare_symbol(name)
        if kind == "BoolConst":
            # Encoded via as_int? No — needs a dedicated accessor. Read kind only.
            # Walk children-less: rely on a future as_bool() accessor, or
            # check via the repr() string. For now, this branch is reachable
            # only via True/False constructors; treat as backend bool literals.
            # TODO: add expr.as_bool() to the inspection API for clean access.
            # As a stopgap, reconstruct via two test exprs.
            raise NotImplementedError(
                "Direct BoolConst translation pending an as_bool() accessor"
            )

        # Composites — fetch children, dispatch by kind.
        children = e.children()

        if kind == "Add":
            return self.backend.add(*[self.to_backend(c) for c in children])
        if kind == "Mul":
            return self.backend.mul(*[self.to_backend(c) for c in children])
        if kind == "Neg":
            return self.backend.neg(self.to_backend(children[0]))
        if kind == "Div":
            return self.backend.div(self.to_backend(children[0]),
                                    self.to_backend(children[1]))
        if kind == "Pow":
            base, exp = children
            exp_int = exp.as_int()
            if exp_int is None:
                raise Unsupported("non-integer exponents not supported")
            return self.backend.pow_int(self.to_backend(base), exp_int)
        if kind == "Eq":
            return self.backend.eq(self.to_backend(children[0]),
                                   self.to_backend(children[1]))
        if kind == "Lt":
            return self.backend.lt(self.to_backend(children[0]),
                                   self.to_backend(children[1]))
        if kind == "Le":
            return self.backend.le(self.to_backend(children[0]),
                                   self.to_backend(children[1]))
        if kind == "Gt":
            return self.backend.gt(self.to_backend(children[0]),
                                   self.to_backend(children[1]))
        if kind == "Ge":
            return self.backend.ge(self.to_backend(children[0]),
                                   self.to_backend(children[1]))
        if kind == "And":
            return self.backend.and_(*[self.to_backend(c) for c in children])
        if kind == "Or":
            return self.backend.or_(*[self.to_backend(c) for c in children])
        if kind == "Not":
            return self.backend.not_(self.to_backend(children[0]))
        if kind == "Implies":
            return self.backend.implies(self.to_backend(children[0]),
                                        self.to_backend(children[1]))
        if kind == "Fn":
            name = e.fn_name()
            assert name is not None
            return self.backend.uninterpreted(name,
                                              [self.to_backend(c) for c in children])

        raise TranslationError(f"unhandled Expr kind: {kind}")

    def _declare_symbol(self, name: str) -> Any:
        sort = self.session.sort_of(name)
        key = (name, sort)
        if key in self._symbols:
            return self._symbols[key]
        if sort == "real":
            ref = self.backend.real(name)
        elif sort == "int":
            ref = self.backend.int(name)
        elif sort == "bool":
            ref = self.backend.bool(name)
        else:
            raise TranslationError(f"unknown sort {sort!r}")
        self._symbols[key] = ref
        return ref
```

- [ ] **Step 2: Add `as_bool` to the inspection API**

In `rust/monomix-py/src/expr.rs`, add:

```rust
fn as_bool(&self) -> Option<bool> {
    let pool = self.pool.lock().expect("pool mutex poisoned");
    match pool.get(self.id) {
        ExprNode::BoolConst(b) => Some(*b),
        _ => None,
    }
}
```

Replace the `BoolConst` branch in `translate.py`:

```python
if kind == "BoolConst":
    b = e.as_bool()
    assert b is not None
    return self.backend.bool_const(b)
```

- [ ] **Step 3: Add the other inspection accessors used by the translator**

`as_int`, `as_rational`, `as_float`, `symbol_name`, `fn_name`, `children` on Expr. In `rust/monomix-py/src/expr.rs` add:

```rust
fn as_int(&self) -> Option<BigInt> {
    let pool = self.pool.lock().expect("pool mutex poisoned");
    match pool.get(self.id) {
        ExprNode::SmallInt(n) => Some(BigInt::from(*n)),
        ExprNode::BigInt(b) => Some((**b).clone()),
        _ => None,
    }
}

fn as_rational(&self) -> Option<(BigInt, BigInt)> {
    let pool = self.pool.lock().expect("pool mutex poisoned");
    match pool.get(self.id) {
        ExprNode::Rational(r) => Some((r.0.clone(), r.1.clone())),
        ExprNode::SmallInt(n) => Some((BigInt::from(*n), BigInt::from(1))),
        ExprNode::BigInt(b) => Some(((**b).clone(), BigInt::from(1))),
        _ => None,
    }
}

fn as_float(&self) -> Option<f64> {
    let pool = self.pool.lock().expect("pool mutex poisoned");
    match pool.get(self.id) {
        ExprNode::Float(f) => Some(f.into_inner()),
        _ => None,
    }
}

fn symbol_name(&self) -> Option<String> {
    let pool = self.pool.lock().expect("pool mutex poisoned");
    match pool.get(self.id) {
        ExprNode::Symbol(s) => Some(pool.str_of(*s).to_string()),
        _ => None,
    }
}

fn fn_name(&self) -> Option<String> {
    let pool = self.pool.lock().expect("pool mutex poisoned");
    match pool.get(self.id) {
        ExprNode::Fn(tag, _) => Some(match tag {
            monomix_kernel::FnTag::Sin => "sin".to_string(),
            monomix_kernel::FnTag::Cos => "cos".to_string(),
            monomix_kernel::FnTag::Tan => "tan".to_string(),
            monomix_kernel::FnTag::Exp => "exp".to_string(),
            monomix_kernel::FnTag::Log => "log".to_string(),
            monomix_kernel::FnTag::Sqrt => "sqrt".to_string(),
            monomix_kernel::FnTag::Abs => "abs".to_string(),
            monomix_kernel::FnTag::Asin => "asin".to_string(),
            monomix_kernel::FnTag::Acos => "acos".to_string(),
            monomix_kernel::FnTag::Atan => "atan".to_string(),
            monomix_kernel::FnTag::Custom(s) => pool.str_of(*s).to_string(),
        }),
        _ => None,
    }
}

fn children(&self) -> Vec<Expr> {
    let pool = self.pool.lock().expect("pool mutex poisoned");
    pool.children(self.id)
        .into_iter()
        .map(|id| Expr::new(Arc::clone(&self.pool), id))
        .collect()
}
```

Update `.pyi`:

```python
class Expr:
    # ... existing ...
    def as_int(self) -> int | None: ...
    def as_rational(self) -> tuple[int, int] | None: ...
    def as_float(self) -> float | None: ...
    def as_bool(self) -> bool | None: ...
    def symbol_name(self) -> str | None: ...
    def fn_name(self) -> str | None: ...
    def children(self) -> list[Expr]: ...
```

- [ ] **Step 4: Rebuild**

```bash
cd python && maturin develop
```

Expected: compile success.

- [ ] **Step 5: Commit**

```bash
git add rust/monomix-py/src/expr.rs python/monomix/smt/translate.py python/monomix/_kernel.pyi
git commit -m "Rewrite SMT translator against Rust-backed Expr"
```

### Task 7.3: Update the Z3 backend to implement the new `Backend` protocol

**Files:**
- Modify: `python/monomix/smt/z3_backend.py`

- [ ] **Step 1: Update imports and the `Z3Backend` class**

The existing `Z3Backend` mixes translator and solver responsibilities. Split: keep the `Z3Backend` solver session (push/pop/assume/decide/prove etc.), and add a separate `Z3TermBuilder` (or extend `Z3Backend`) that implements the `Backend` protocol from `translate.py`.

Replace `Z3Backend` with a version that owns a `Translator` instance pointing at a `Z3TermBuilder`:

```python
"""Z3 backend for the SMT bridge.

Implements the Backend protocol from translate.py and provides the
session interface (push/pop/assume/decide/prove/declared_symbols)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from monomix import Expr, Session as MonomixSession

from .errors import BackendUnavailable, SolverError, Unsupported
from .translate import Translator

try:
    import z3  # type: ignore
except ImportError as e:
    z3 = None  # noqa: N816
    _IMPORT_ERROR = e
else:
    _IMPORT_ERROR = None


def _require_z3():
    if z3 is None:
        raise BackendUnavailable(
            "z3-solver is not installed. Install with `pip install z3-solver`."
        ) from _IMPORT_ERROR


# ----------------------------------------------------------------------
# Result types
# ----------------------------------------------------------------------

@dataclass
class Proved: ...
@dataclass
class Refuted:
    counterexample: dict[str, Any]
@dataclass
class Sat:
    model: dict[str, Any]
@dataclass
class Unsat: ...
@dataclass
class Unknown: ...

ProveResult = Proved | Refuted | Unknown
DecideResult = Sat | Unsat | Unknown


# ----------------------------------------------------------------------
# Term builder implementing the Backend protocol
# ----------------------------------------------------------------------

class Z3TermBuilder:
    def real(self, name: str) -> Any:
        return z3.Real(name)
    def int(self, name: str) -> Any:
        return z3.Int(name)
    def bool(self, name: str) -> Any:
        return z3.Bool(name)
    def rational_const(self, num: int, den: int) -> Any:
        return z3.RealVal(num) if den == 1 else z3.Q(num, den)
    def int_const(self, n: int) -> Any:
        return z3.IntVal(n)
    def bool_const(self, b: bool) -> Any:
        return z3.BoolVal(b)
    def add(self, *xs: Any) -> Any:
        if not xs: return z3.RealVal(0)
        out = xs[0]
        for x in xs[1:]:
            out = out + x
        return out
    def mul(self, *xs: Any) -> Any:
        if not xs: return z3.RealVal(1)
        out = xs[0]
        for x in xs[1:]:
            out = out * x
        return out
    def neg(self, x: Any) -> Any: return -x
    def div(self, a: Any, b: Any) -> Any: return a / b
    def pow_int(self, base: Any, n: int) -> Any:
        if n == 0: return z3.RealVal(1)
        if n < 0:
            return z3.RealVal(1) / self.pow_int(base, -n)
        out = base
        for _ in range(n - 1):
            out = out * base
        return out
    def eq(self, a: Any, b: Any) -> Any: return a == b
    def lt(self, a: Any, b: Any) -> Any: return a < b
    def le(self, a: Any, b: Any) -> Any: return a <= b
    def gt(self, a: Any, b: Any) -> Any: return a > b
    def ge(self, a: Any, b: Any) -> Any: return a >= b
    def and_(self, *xs: Any) -> Any:
        return z3.And(*xs) if xs else z3.BoolVal(True)
    def or_(self, *xs: Any) -> Any:
        return z3.Or(*xs) if xs else z3.BoolVal(False)
    def not_(self, x: Any) -> Any: return z3.Not(x)
    def implies(self, a: Any, b: Any) -> Any: return z3.Implies(a, b)
    def uninterpreted(self, name: str, args: list[Any]) -> Any:
        domain = [z3.RealSort()] * len(args)
        decl = z3.Function(name, *domain, z3.RealSort())
        return decl(*args)


# ----------------------------------------------------------------------
# Solver session
# ----------------------------------------------------------------------

class Z3Backend:
    def __init__(self, session: MonomixSession, *, default_timeout_ms: int = 5000) -> None:
        _require_z3()
        self._monomix_session = session
        self._solver = z3.Solver()
        self._solver.set("timeout", default_timeout_ms)
        self._builder = Z3TermBuilder()
        self._translator = Translator(self._builder, session)

    def assume(self, e: Expr) -> None:
        self._solver.add(self._translator.to_backend(e))

    def push(self) -> None:
        self._solver.push()

    def pop(self) -> None:
        self._solver.pop()

    def declared_symbols(self) -> list[str]:
        return [name for (name, _sort) in self._translator._symbols.keys()]

    def decide(self, formula: Expr) -> DecideResult:
        self._solver.push()
        try:
            self._solver.add(self._translator.to_backend(formula))
            r = self._solver.check()
            if r == z3.sat:
                model = self._solver.model()
                return Sat(model=_extract_model(model))
            if r == z3.unsat:
                return Unsat()
            return Unknown()
        finally:
            self._solver.pop()

    def prove(self, claim: Expr, *, assumptions: list[Expr] | None = None) -> ProveResult:
        self._solver.push()
        try:
            for a in assumptions or []:
                self._solver.add(self._translator.to_backend(a))
            # Try to refute the claim
            self._solver.add(z3.Not(self._translator.to_backend(claim)))
            r = self._solver.check()
            if r == z3.unsat:
                return Proved()
            if r == z3.sat:
                model = self._solver.model()
                return Refuted(counterexample=_extract_model(model))
            return Unknown()
        finally:
            self._solver.pop()


def _extract_model(model: Any) -> dict[str, Any]:
    from fractions import Fraction
    out: dict[str, Any] = {}
    for d in model:
        v = model[d]
        if z3.is_int_value(v):
            out[str(d)] = v.as_long()
        elif z3.is_rational_value(v):
            out[str(d)] = Fraction(v.numerator_as_long(), v.denominator_as_long())
        elif z3.is_bool(v):
            out[str(d)] = bool(v)
        else:
            out[str(d)] = v
    return out
```

- [ ] **Step 2: Update `python/monomix/smt/__init__.py`**

```python
"""SMT bridge — translate monomix Expr into a backend solver."""

from __future__ import annotations

from contextlib import contextmanager
from typing import Iterator

from monomix import Session

from .errors import BackendUnavailable, SolverError, TranslationError, Unsupported
from .z3_backend import (
    DecideResult,
    ProveResult,
    Proved,
    Refuted,
    Sat,
    Unknown,
    Unsat,
    Z3Backend,
)

__all__ = [
    "open_session",
    "Z3Backend",
    "Proved",
    "Refuted",
    "Unknown",
    "Sat",
    "Unsat",
    "ProveResult",
    "DecideResult",
    "SolverError",
    "BackendUnavailable",
    "TranslationError",
    "Unsupported",
]


@contextmanager
def open_session(monomix_session: Session, *, default_timeout_ms: int = 5000) -> Iterator[Z3Backend]:
    """Open an SMT session bound to a monomix Session.

    The SMT session reads sort declarations from `monomix_session`.
    """
    backend = Z3Backend(monomix_session, default_timeout_ms=default_timeout_ms)
    try:
        yield backend
    finally:
        pass
```

- [ ] **Step 3: Rebuild and ensure imports work**

```bash
cd python && maturin develop && python -c "from monomix.smt import open_session, Proved; print('ok')"
```

Expected: prints `ok`.

- [ ] **Step 4: Commit**

```bash
git add python/monomix/smt/__init__.py python/monomix/smt/z3_backend.py
git commit -m "Adapt Z3 backend to the new Backend protocol and Rust-backed Expr"
```

### Task 7.4: Port tests from `test_solver.py` to `test_smt.py`

**Files:**
- Delete: `python/tests/test_solver.py`
- Create: `python/tests/test_smt.py`

- [ ] **Step 1: Write the new test file**

```python
"""SMT bridge tests, rewritten against the Rust-backed Expr."""

from __future__ import annotations

from fractions import Fraction

import pytest

from monomix import Session
from monomix.smt import (
    BackendUnavailable,
    Proved,
    Refuted,
    Sat,
    Unknown,
    Unsat,
    Unsupported,
    open_session,
)

try:
    import z3  # noqa: F401
except ImportError:
    pytest.skip("z3-solver not installed", allow_module_level=True)


# -- Session lifecycle ------------------------------------------------------

def test_smt_session_opens_and_closes():
    s = Session()
    with open_session(s) as smt:
        assert smt.declared_symbols() == []


# -- Linear real arithmetic ------------------------------------------------

def test_prove_simple_linear_inequality():
    s = Session()
    x, y = s.symbol("x"), s.symbol("y")
    zero = s.integer(0)
    with open_session(s) as smt:
        result = smt.prove((x + y) > zero, assumptions=[x > zero, y > zero])
        assert isinstance(result, Proved)


def test_refute_with_counterexample():
    s = Session()
    x = s.symbol("x")
    with open_session(s) as smt:
        result = smt.prove(x > s.integer(1), assumptions=[x > s.integer(0)])
        assert isinstance(result, Refuted)
        cx = result.counterexample
        assert "x" in cx
        val = cx["x"]
        assert val > 0
        assert val <= 1


# -- Nonlinear real arithmetic ---------------------------------------------

def test_square_is_nonneg():
    s = Session()
    x = s.symbol("x")
    with open_session(s) as smt:
        result = smt.prove(x ** s.integer(2) >= s.integer(0))
        assert isinstance(result, Proved)


def test_unit_disk_intersect_halfplane_is_satisfiable():
    s = Session()
    x, y = s.symbol("x"), s.symbol("y")
    formula = ((x ** s.integer(2) + y ** s.integer(2)) < s.integer(1)) & \
              ((x + y) > s.rational(1, 2))
    with open_session(s) as smt:
        result = smt.decide(formula)
        assert isinstance(result, Sat)
        xv, yv = result.model["x"], result.model["y"]
        assert xv * xv + yv * yv < 1
        assert xv + yv > Fraction(1, 2)


def test_unit_disk_disjoint_from_far_halfplane():
    s = Session()
    x, y = s.symbol("x"), s.symbol("y")
    formula = ((x ** s.integer(2) + y ** s.integer(2)) < s.integer(1)) & \
              ((x + y) > s.integer(10))
    with open_session(s) as smt:
        result = smt.decide(formula)
        assert isinstance(result, Unsat)


# -- Push/pop --------------------------------------------------------------

def test_push_pop_isolates_assumptions():
    s = Session()
    x = s.symbol("x")
    with open_session(s) as smt:
        smt.assume(x > s.integer(0))
        smt.push()
        smt.assume(x < s.integer(0))
        # Inner scope: x > 0 and x < 0 → Unsat
        assert isinstance(smt.decide(x == x), Unsat)
        smt.pop()
        # Outer scope: only x > 0
        assert isinstance(smt.prove(x > s.integer(0)), Proved)


# -- Integer sort ----------------------------------------------------------

def test_integer_division_property():
    s = Session()
    s.declare("n", "int")
    n = s.symbol("n")
    with open_session(s) as smt:
        result = smt.prove((n + n) == (s.integer(2) * n))
        assert isinstance(result, Proved)


# -- Unsupported / uninterpreted ------------------------------------------

def test_symbolic_exponent_is_unsupported():
    s = Session()
    x, y = s.symbol("x"), s.symbol("y")
    with open_session(s) as smt:
        with pytest.raises(Unsupported):
            smt.prove((x ** y) == (x ** y))


def test_unknown_function_becomes_uninterpreted():
    s = Session()
    x = s.symbol("x")
    # Build sin(x) via the parser (kernel knows sin as a FnTag)
    sin_x = s.parse("sin(x)")
    with open_session(s) as smt:
        result = smt.prove(sin_x == sin_x)
        assert isinstance(result, Proved)


# -- Cross-session refusal -------------------------------------------------

def test_cross_session_expr_raises_in_smt():
    from monomix import CrossSessionError
    s1 = Session()
    s2 = Session()
    x = s1.symbol("x")
    y = s2.symbol("y")
    with pytest.raises(CrossSessionError):
        _ = x < y  # caught at operator level before reaching SMT


# -- Backend availability --------------------------------------------------

def test_backend_unavailable_is_an_exception_class():
    assert issubclass(BackendUnavailable, Exception)
```

- [ ] **Step 2: Delete the old test file**

```bash
git rm python/tests/test_solver.py
```

- [ ] **Step 3: Run tests**

```bash
cd python && pytest tests/test_smt.py -v
```

Expected: green.

- [ ] **Step 4: Commit**

```bash
git add python/tests/test_smt.py python/tests/test_solver.py
git commit -m "Port SMT bridge tests to test_smt.py against Rust-backed Expr"
```

### Task 7.5: Final full-suite verification

- [ ] **Step 1: Run everything**

```bash
cd rust/monomix-kernel && cargo test && cd ../.. && cd python && pytest -v
```

Expected: all green.

- [ ] **Step 2: Run lints**

```bash
cd rust/monomix-py && cargo clippy --all-targets -- -D warnings
cd ../monomix-kernel && cargo clippy --all-targets -- -D warnings
cd ../../python && pyright --strict monomix tests
```

Expected: clean.

- [ ] **Step 3: Commit (no-op if clean)**

If lints found nothing to fix, no commit. Otherwise fix and commit.

---

## Phase 8: Docs and doc-debt notes

### Task 8.1: Write `docs/python-bindings.md`

**Files:**
- Create: `docs/python-bindings.md`

- [ ] **Step 1: Write the page**

```markdown
# Python bindings

The `monomix` package exposes the Rust kernel through PyO3. The user-facing types are `monomix.Expr` and `monomix.Session`.

## Quick start

\`\`\`python
from monomix import Session, simplify, df

s = Session()
x = s.symbol("x")
expr = x ** s.integer(3)
print(simplify(df(expr, x)))   # 3*x^2
\`\`\`

## Session

A `Session` owns the underlying expression pool. Every `Expr` produced from a session keeps the pool alive, so the `Expr` outlives the session:

\`\`\`python
def make():
    s = Session()
    return s.symbol("x")     # still usable after make() returns
\`\`\`

Mixing `Expr` from two different `Session`s raises `CrossSessionError`.

## Operator surface

| Operator | Builds |
|----------|--------|
| `+ - * / ** -` | arithmetic node |
| `==` `!=` | `Eq`, `Not(Eq(...))` |
| `<` `<=` `>` `>=` | `Lt`, `Le`, `Gt`, `Ge` |
| `& \| ~` | `And`, `Or`, `Not` |

### `==` returns an expression, not a bool

\`\`\`python
e = (x == 0)          # Eq(x, 0), an Expr
bool(e)               # False (handle equality on x and 0)
hash(e)               # hashable
\`\`\`

For any non-`Eq` expression, `bool(...)` raises `TypeError`. Use `e.is_same(other)` for guaranteed-bool handle equality.

### Operator precedence trap

Python's `&` and `|` bind tighter than `==`. Parenthesize:

\`\`\`python
bad = a == b & c == d       # parses as a == (b & c) == d
good = (a == b) & (c == d)  # what you wanted
\`\`\`

## Errors

| Exception | When |
|-----------|------|
| `ParseError` | parser failure (with `.span = (start, end)`) |
| `EvalError` | unbound symbol, division by zero, overflow |
| `UnsupportedError` | feature not in Phase 1 (e.g. `df` of a `Lt`) |
| `CrossSessionError` | mixing `Expr` from different `Session`s |
| `MonomixError` | base class for all of the above |
```

- [ ] **Step 2: Commit**

```bash
git add docs/python-bindings.md
git commit -m "Add Python bindings user-facing docs page"
```

### Task 8.2: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update the "active code" paragraph**

Find the paragraph that says "the only active code is rust/monomix-kernel/..." and replace with:

```markdown
The active code is split across:

- `rust/monomix-kernel/` — Phase 1 (MVP) symbolic kernel.
- `rust/monomix-py/` — PyO3 binding crate exposing the kernel to Python.
- `python/monomix/` — Python package. Provides `monomix.Expr` (Rust-backed handle), `monomix.Session`, module-level kernel functions, and the SMT bridge under `monomix.smt`.
- `rust/solver-bridge/` — Phase 2 sketch; **not buildable** yet (Z3 deps commented out). Don't try to build it.
```

- [ ] **Step 2: Update Python tests row in the Common commands table**

Replace:

```
| Python tests (SMT bridge only today) | `cd python && pip install -e .[dev] && pytest` |
```

With:

```
| Build Python bindings (dev loop) | `cd python && maturin develop` |
| Python tests (Expr, Session, kernel calls, SMT bridge) | `cd python && pytest` |
```

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "Update CLAUDE.md for Python bindings"
```

### Task 8.3: Doc-debt follow-up list

**Files:**
- Create: `docs/superpowers/specs/2026-05-13-python-bindings-followups.md`

- [ ] **Step 1: Write the follow-up list**

```markdown
# Python bindings — follow-up notes

Doc-debt and deferred items surfaced while implementing the python-bindings spec.

## ADR-0002 inconsistencies

- `decisions/0002-high-level-architecture.md` says expression handles are `Arc<ExprNode>`; the kernel actually uses an arena pool with `ExprId` handles. The PyO3 boundary therefore holds `(Arc<Mutex<ExprPool>>, ExprId)`, not `Arc<ExprNode>`.
- ADR-0002 says the crate layout is `crates/monomix-kernel/` and `crates/monomix-py/`; the actual layout is `rust/monomix-kernel/` and `rust/monomix-py/`.

**Action:** write a follow-up ADR amending these two points, citing this work as the reason for the correction.

## Deferred Phase 1 items

The Python bindings work does not include:

- Plugin entry-point discovery (Phase 1 §1.10).
- CLI / REPL (Phase 1 §1.9).
- CI wheel matrix (SCOPE §0.9 — needs its own spec).
- Sphinx / Read the Docs setup.

## Out-of-scope items called out during brainstorming

- Reverse `model → Expr` reconstruction in the SMT bridge (it currently returns raw Python `int` / `Fraction`).
- Additional SMT backends beyond Z3.
- REDUCE-syntax extensions for inequalities / boolean operators (the new kernel variants are only reachable via Python constructors).

## Known design hazards documented in the user-facing docs

- Operator precedence with `==` vs `&` / `|`. Documented; no automated check.
- `__bool__` of non-`Eq` expressions raises. Aligned with NumPy convention; documented.
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/specs/2026-05-13-python-bindings-followups.md
git commit -m "Record python-bindings follow-up items and doc-debt"
```

---

## Final verification

- [ ] **Run the entire suite**

```bash
cd rust/monomix-kernel && cargo test && cargo clippy --all-targets -- -D warnings
cd ../monomix-py && cargo clippy --all-targets -- -D warnings
cd ../../python && maturin develop && pytest -v && pyright --strict monomix tests
```

Expected: all green.

- [ ] **Sanity check the public API end-to-end**

```bash
python -c "
from monomix import Session, simplify, df, evaluate_numeric
from monomix.smt import open_session, Proved

s = Session()
x = s.symbol('x')
e = simplify(df(x ** s.integer(3), x))
print('df(x^3)/dx =', repr(e))

with open_session(s) as smt:
    result = smt.prove((x * x) >= s.integer(0))
    assert isinstance(result, Proved)
print('Square-is-nonneg proved.')
"
```

Expected: prints the derivative expression and `Square-is-nonneg proved.`

- [ ] **Final commit (if any leftover changes)**

If `git status` shows untracked or unstaged changes from the verification, commit them with a clean-up message. Otherwise, no commit.
