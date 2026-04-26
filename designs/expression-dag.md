# Expression DAG — System Design

**Component:** `monomix-kernel::expr`
**Status:** Design phase
**Date:** 2026-04-26
**References:** SCOPE.md §0.2, §1.1; ADR-0001; ADR-0002

---

## 1. Requirements

### 1.1 Functional requirements

The expression DAG is the central data structure of the Monomix kernel. Every other kernel component — the parser, simplifier, differentiator, polynomial engine, solver, and substitution engine — produces and consumes expressions through this representation.

It must support:

- **Atoms:** arbitrary-precision integers, exact rationals, IEEE-754 floats, symbols (variable names), and string literals.
- **Composite nodes:** addition (n-ary), multiplication (n-ary), exponentiation (binary), negation (unary), division (binary), equality (binary), function application (tag + n-ary args), and lists (n-ary).
- **Structural sharing:** identical subexpressions are stored once. `x^2 + x^2` allocates one `Pow(x, 2)` node, not two.
- **Cheap equality:** two expressions built from the same structure must compare equal in O(1) time.
- **Immutability:** once created, a node is never modified. Transformations produce new nodes.
- **Traversal:** depth-first iteration over children, with the ability to map over subexpressions (used by `simplify`, `df`, `substitute`).

### 1.2 Non-functional requirements

| Requirement | Target | Rationale |
|-------------|--------|-----------|
| Node creation | <200 ns per interned node | The simplifier creates thousands of nodes per operation |
| Equality check | O(1) — integer comparison | Used in every simplification rule match |
| Memory per node | ≤64 bytes median | Large expressions (10K+ nodes) must fit in L2 cache |
| Thread safety | `Send + Sync` | MCP server needs concurrent access after GIL release |
| PyO3 boundary overhead | <500 ns per `Expr` method call | SCOPE.md benchmark target |

### 1.3 Constraints

- No `unsafe` in this module (allowed only in `monomix-py` for PyO3 glue).
- No heap allocation on the equality-check fast path.
- No GMP/LGPL dependency — use `num-bigint` (pure Rust).
- The pool is not shared across sessions in Phase 1 (revisited in Phase 2 for MCP caching).

---

## 2. High-Level Design

### 2.1 Core types

```rust
/// A lightweight handle to an expression inside an ExprPool.
/// Copy, Eq, Hash — 4 bytes. This is what every kernel function passes around.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(u32);

/// An interned string handle. Same Copy/Eq/Hash properties as ExprId.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct InternedStr(u32);

/// Tag for built-in functions (avoids string comparison in hot paths).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum FnTag {
    Sin, Cos, Tan, Exp, Log, Sqrt, Abs,
    Asin, Acos, Atan,
    /// User-defined or plugin-registered function, identified by name.
    Custom(InternedStr),
}

/// The expression node enum. Stored inside ExprPool's arena.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum ExprNode {
    // --- Atoms ---
    Integer(BigInt),
    Rational(BigInt, BigInt),    // numerator, denominator (always in lowest terms)
    Float(OrderedFloat<f64>),   // OrderedFloat for Eq + Hash
    Symbol(InternedStr),
    String(InternedStr),

    // --- Composite ---
    Add(Vec<ExprId>),           // n-ary, flattened: Add([a, b, c]) not Add([Add([a,b]), c])
    Mul(Vec<ExprId>),           // n-ary, flattened
    Pow(ExprId, ExprId),        // base, exponent
    Neg(ExprId),                // unary minus
    Div(ExprId, ExprId),        // numerator, denominator
    Eq(ExprId, ExprId),         // equation lhs = rhs
    Fn(FnTag, Vec<ExprId>),     // function application
    List(Vec<ExprId>),          // ordered collection
}
```

### 2.2 ExprPool — the interning table

```rust
pub struct ExprPool {
    /// Arena of all expression nodes. ExprId indexes into this.
    nodes: Vec<ExprNode>,

    /// Deduplication map: content hash → ExprId.
    /// Uses pre-computed hashes to avoid rehashing on lookup.
    dedup: HashMap<u64, SmallVec<[ExprId; 1]>>,

    /// String interning table: string content → InternedStr.
    strings: IndexSet<String>,

    /// Pre-interned common constants for fast access.
    pub zero: ExprId,
    pub one: ExprId,
    pub minus_one: ExprId,
}
```

The pool is the single owner of all expression data. Every kernel function takes `&ExprPool` (read) or `&mut ExprPool` (create nodes). The pool is never cloned — it's wrapped in `Arc<Mutex<ExprPool>>` only at the PyO3 boundary.

### 2.3 Interning flow

```
caller: pool.intern(ExprNode::Add(vec![a, b]))
  │
  ▼
1. Compute content hash of the node (children are ExprIds, already interned)
  │
  ▼
2. Look up hash in dedup map
  │
  ├── HIT: compare node structurally with candidates → return existing ExprId
  │
  └── MISS: push node onto arena → record ExprId in dedup map → return new ExprId
```

Because `ExprId` is `Copy` and 4 bytes, passing expressions around is no more expensive than passing an integer. Structural equality between two expressions is `a == b` — a single `u32` comparison — because identical structures always resolve to the same `ExprId` through interning.

### 2.4 Data flow through the kernel

```
                   source string
                        │
                        ▼
                   ┌─────────┐
                   │  Parser  │  produces ExprIds via pool.intern()
                   └────┬────┘
                        │ ExprId (root of AST)
                        ▼
              ┌─────────────────────┐
              │  Simplifier / Diff  │  reads nodes via pool[id],
              │  / Solve / Subst    │  creates new nodes via pool.intern()
              └─────────┬──────────┘
                        │ ExprId (result)
                        ▼
                ┌───────────────┐
                │  PyO3 boundary │  wraps ExprId + Arc<Mutex<ExprPool>>
                └───────┬───────┘      into ExprHandle
                        │
                        ▼
                  Python Expr object
```

Every kernel operation is: read from pool → compute → intern result into pool → return `ExprId`. The pool grows monotonically during a session. No garbage collection in Phase 1 (see §5 for future work).

---

## 3. Deep Dive

### 3.1 Interning guarantees and invariants

**Invariant 1 — Structural uniqueness:** For any two `ExprId` values `a` and `b`, `a == b` if and only if `pool.nodes[a]` and `pool.nodes[b]` are structurally identical (recursively).

**Invariant 2 — Canonical children:** Composite nodes store children that are themselves interned `ExprId`s. This means the hash of a composite node depends only on the variant tag and the child `ExprId` values (integers), not on recursive structure. Hashing is O(fan-out), not O(tree-size).

**Invariant 3 — Normalized rationals:** `Rational(p, q)` always has `q > 0` and `gcd(p, q) == 1`. This is enforced by a `rational()` constructor on `ExprPool` that normalizes before interning.

**Invariant 4 — Flattened associative operators:** `Add` and `Mul` are n-ary and flattened: `Add([a, Add([b, c])])` is never created — the pool's `add()` constructor flattens it to `Add([a, b, c])`. This simplifies pattern matching in the simplifier. Children within `Add` and `Mul` are stored in a canonical sort order (by `ExprId` value) to ensure that `a + b` and `b + a` intern to the same node.

**Invariant 5 — No redundant wrappers:** `Neg(Neg(x))` is never created — the `neg()` constructor returns `x`. `Div(a, Integer(1))` returns `a`. `Pow(x, Integer(1))` returns `x`. `Pow(x, Integer(0))` returns `one`. These normalizations happen at interning time, not in the simplifier.

### 3.2 Hashing strategy

Content hashing uses `FxHasher` (non-cryptographic, fast) with a discriminant-tagged scheme:

```rust
fn content_hash(node: &ExprNode) -> u64 {
    let mut h = FxHasher::default();
    // Discriminant tag prevents collisions across variants
    std::mem::discriminant(node).hash(&mut h);
    match node {
        ExprNode::Integer(n) => n.hash(&mut h),
        ExprNode::Symbol(s) => s.hash(&mut h),
        ExprNode::Add(children) => {
            // Children are sorted ExprIds — hash in order
            for c in children { c.hash(&mut h); }
        }
        // ... etc for each variant
    }
    h.finish()
}
```

Hash collisions are handled by the `SmallVec<[ExprId; 1]>` in the dedup map — on collision, linear scan with structural equality. In practice, `FxHasher` on small integer sequences produces very few collisions.

### 3.3 String interning

Symbol and string literal text is stored once in an `IndexSet<String>`. `InternedStr(u32)` is the index. Benefits:

- Symbol equality is integer comparison.
- No string allocation after the first occurrence.
- `IndexSet` preserves insertion order, which gives deterministic iteration if needed.

Common symbols (`x`, `y`, `z`, `t`, `e`, `pi`, `i`) are pre-interned at pool construction for fast access.

### 3.4 Memory layout

Target: ≤64 bytes median per node.

```
ExprNode::Integer(BigInt)     — 32 bytes (BigInt inline for small values via smallvec)
ExprNode::Rational(BigInt×2)  — 64 bytes
ExprNode::Float(f64)          — 16 bytes (discriminant + f64)
ExprNode::Symbol(u32)         — 8 bytes
ExprNode::Add(Vec<ExprId>)    — 32 bytes (Vec header; children are u32 each)
ExprNode::Pow(ExprId×2)       — 12 bytes
ExprNode::Fn(FnTag, Vec)      — 36 bytes
```

The `nodes: Vec<ExprNode>` arena gives dense, cache-friendly storage. Traversal is a sequence of index lookups into a contiguous array — no pointer chasing through heap allocations.

### 3.5 Constructor API

The pool exposes typed constructors that enforce invariants at creation time. Callers never construct `ExprNode` directly.

```rust
impl ExprPool {
    // --- Atoms ---
    pub fn integer(&mut self, n: impl Into<BigInt>) -> ExprId;
    pub fn rational(&mut self, p: BigInt, q: BigInt) -> ExprId;  // normalizes
    pub fn float(&mut self, f: f64) -> ExprId;
    pub fn symbol(&mut self, name: &str) -> ExprId;
    pub fn string(&mut self, s: &str) -> ExprId;

    // --- Composite (normalizing) ---
    pub fn add(&mut self, children: Vec<ExprId>) -> ExprId;      // flattens + sorts
    pub fn mul(&mut self, children: Vec<ExprId>) -> ExprId;      // flattens + sorts
    pub fn pow(&mut self, base: ExprId, exp: ExprId) -> ExprId;  // x^0→1, x^1→x
    pub fn neg(&mut self, x: ExprId) -> ExprId;                  // neg(neg(x))→x
    pub fn div(&mut self, num: ExprId, den: ExprId) -> ExprId;   // x/1→x
    pub fn eq(&mut self, lhs: ExprId, rhs: ExprId) -> ExprId;
    pub fn func(&mut self, tag: FnTag, args: Vec<ExprId>) -> ExprId;
    pub fn list(&mut self, items: Vec<ExprId>) -> ExprId;

    // --- Access ---
    pub fn get(&self, id: ExprId) -> &ExprNode;
    pub fn children(&self, id: ExprId) -> &[ExprId];  // empty for atoms

    // --- Queries ---
    pub fn is_zero(&self, id: ExprId) -> bool;
    pub fn is_one(&self, id: ExprId) -> bool;
    pub fn is_atom(&self, id: ExprId) -> bool;
    pub fn is_numeric(&self, id: ExprId) -> bool;     // Integer | Rational | Float
    pub fn contains_symbol(&self, expr: ExprId, sym: ExprId) -> bool;

    // --- Low-level ---
    fn intern(&mut self, node: ExprNode) -> ExprId;   // private: hash + dedup
}
```

### 3.6 Traversal and transformation

Two core patterns used by every kernel module:

**Pattern 1 — Recursive map (bottom-up):**

```rust
/// Apply `f` to every subexpression, bottom-up. Returns a new ExprId.
/// Memoized: each unique subexpression is transformed at most once.
pub fn map_bottom_up(
    pool: &mut ExprPool,
    root: ExprId,
    f: &mut dyn FnMut(&mut ExprPool, ExprId) -> ExprId,
) -> ExprId {
    let mut cache: HashMap<ExprId, ExprId> = HashMap::new();
    map_impl(pool, root, f, &mut cache)
}
```

Used by: `substitute`, `simplify`, `differentiate` (for the chain rule's inner derivative).

**Pattern 2 — Fold (top-down accumulator):**

```rust
/// Fold over the DAG with an accumulator. Each node is visited once.
pub fn fold<A>(
    pool: &ExprPool,
    root: ExprId,
    init: A,
    f: &mut dyn FnMut(A, ExprId, &ExprNode) -> A,
) -> A;
```

Used by: `contains_symbol`, `free_variables`, `evaluate_numeric`.

Both patterns use a visited set keyed on `ExprId` to avoid redundant work on shared subexpressions — a DAG walk, not a tree walk. This is where hash-consing pays off: a shared subtree of depth `d` is visited once, not `2^d` times.

### 3.7 PyO3 boundary

The Python `Expr` object holds an `ExprHandle`:

```rust
/// Sent across the PyO3 boundary. Keeps the pool alive via Arc.
#[derive(Clone)]
pub struct ExprHandle {
    pool: Arc<Mutex<ExprPool>>,
    id: ExprId,
}
```

Python method calls on `Expr` lock the pool, perform the operation, and return a new `ExprHandle`. The `Mutex` is held only during the operation — not across Python statements. The GIL is released before acquiring the mutex for operations >1 ms.

```python
# Python usage — user never sees ExprId or ExprPool
x = monomix.symbol("x")
expr = x**2 + 3*x + 1       # __pow__, __mul__, __add__ each lock/unlock pool
result = monomix.simplify(expr)
```

### 3.8 Error handling

The expression module itself is mostly infallible — interning a well-formed node cannot fail. Errors arise at the boundary:

| Error | Source | Handling |
|-------|--------|----------|
| Integer overflow in `ExprId` | >2^32 nodes in one pool | Return `KernelError::PoolExhausted`. Unlikely in practice (4B nodes ≈ 256 GB). |
| Division by zero in `rational()` | User input `1/0` | Return `KernelError::DivisionByZero` with source span. |
| NaN in `float()` | User input or computation | Store as-is; `is_numeric()` returns false for NaN. |

---

## 4. Trade-off Analysis

### 4.1 Arena + ExprId vs. Arc<ExprNode> pointers

**Chosen: Arena + ExprId.** Expressions are `u32` indices into a `Vec<ExprNode>`.

| Dimension | Arena + ExprId | Arc\<ExprNode\> |
|-----------|---------------|----------------|
| Size of a handle | 4 bytes (u32) | 8 bytes (pointer) + 8 bytes (refcount) |
| Equality check | u32 comparison | Pointer comparison (same cost, but only after interning) |
| Cache locality | Excellent — nodes packed in Vec | Poor — nodes scattered on heap |
| Deallocation | Not needed (pool owns all) | Reference counting, potential cycles |
| Thread safety | Pool behind Mutex | Each Arc is independently Send+Sync |
| Complexity | Must pass `&pool` everywhere | Self-contained nodes |

The main downside of arenas is the "`&pool` everywhere" ergonomic cost. Every function that reads or creates expressions needs a pool reference. This is acceptable because the kernel already threads `&mut ExprPool` through all operations by design — the pool is the session's working memory.

The `Arc<ExprNode>` approach is simpler ergonomically (self-contained values) but loses cache locality and makes the deduplication map more complex (must store and compare full `Arc`s). It also risks reference-counting cycles if expressions ever become mutually recursive (they don't currently, but the arena approach makes this impossible by construction).

### 4.2 Sorting children for canonical form vs. insertion order

**Chosen: Canonical sort order (by ExprId value) for Add and Mul children.**

This means `a + b` and `b + a` are the same node after interning. The simplifier doesn't need to handle commutativity — it's baked into the representation.

Trade-off: the sort costs O(n log n) per `add()`/`mul()` call. This is paid once at interning time and saves repeated normalization in the simplifier. For typical expressions (2-10 children), the sort is negligible.

Alternative considered: keep insertion order and handle commutativity in the simplifier. Rejected because it makes every simplification rule more complex and equality checking more expensive (must compare sorted vs. sorted anyway).

### 4.3 Flattening vs. binary operators

**Chosen: N-ary flattened Add and Mul.**

`a + b + c` is `Add([a, b, c])`, not `Add([Add([a, b]), c])`. This simplifies like-term collection (scan one flat list) and produces smaller, flatter DAGs.

Trade-off: the flatten step in the `add()`/`mul()` constructors must recursively check children. A deeply nested expression like `((a + b) + c) + d` triggers flattening at each level. This is amortized — each node is flattened once at creation.

Alternative considered: binary-only operators (classic tree). Rejected because binary trees for addition/multiplication are deeper, harder to pattern-match in the simplifier, and produce more nodes (2n-1 nodes for n terms vs. 1 node for n terms).

### 4.4 Eager normalization vs. lazy normalization

**Chosen: Eager normalization at interning time** (Neg(Neg(x))→x, Pow(x,1)→x, etc.).

Trade-off: every `intern()` call does a small amount of extra work. But this work is done once and saves the simplifier from handling trivial cases on every pass. It also makes the interning guarantee stronger — there are fewer distinct representations of the same mathematical object.

What is not normalized eagerly: trigonometric identities, like-term collection beyond trivial cases, algebraic simplification. Those belong to the simplifier.

---

## 5. Scale, Limits, and Future Work

### 5.1 Pool growth and memory

The pool grows monotonically in Phase 1. Nodes are never deallocated. For a typical interactive session (thousands of expressions), this is fine — the pool stays under a few MB.

For long-running MCP server sessions (Phase 1.5+), pool growth may become an issue. Options for future phases:

- **Generational pool:** partition nodes into generations; collect old generations when no live `ExprId` references them. Requires tracking which `ExprId`s are reachable from Python.
- **Session-scoped pools:** each MCP request gets its own pool; results are copied into a shared output pool. Avoids garbage collection entirely.
- **Copy-on-access from a frozen pool:** freeze the pool after each request; next request works on a fresh pool that can look up (but not modify) the frozen one. Avoids full copies.

This is explicitly deferred — Phase 1 ships without GC, and the design revisit happens before Phase 1.5 (MCP) based on measured pool sizes.

### 5.2 Parallel simplification

The current design uses `Mutex<ExprPool>` at the PyO3 boundary, serializing pool mutations. For Phase 2+, parallel simplification of independent subexpressions is desirable. Options:

- **Read-write lock:** `RwLock<ExprPool>` allows concurrent reads (traversal) with exclusive writes (interning). Most operations are read-heavy.
- **Per-thread local pools with merge:** each thread interns locally, results are merged into the shared pool. Avoids contention entirely but requires a merge step.
- **Lock-free arena:** append-only `Vec` with atomic length counter. Reads are safe without locking; writes use `compare_exchange` on the length. The dedup map is the bottleneck — requires a concurrent hashmap (e.g., `dashmap`).

Deferred to Phase 2. The `Mutex` is sufficient for Phase 1 and Phase 1.5 (GIL-released operations are individually single-threaded; concurrency is across requests, not within one).

### 5.3 Serialization

For Phase 2 (result caching, script loading), expressions need serialization. The arena-based design makes this natural: serialize the `nodes` vec and `strings` set; `ExprId` values are stable indices. A compact binary format (e.g., `postcard` or `bincode`) can serialize/deserialize a pool in a single pass.

---

## 6. Testing Strategy

### 6.1 Unit tests (cargo test)

- **Interning roundtrip:** `pool.integer(42)` returns the same `ExprId` on repeated calls.
- **Structural uniqueness:** `pool.add(vec![a, b]) == pool.add(vec![b, a])` (commutativity via sort).
- **Flattening:** `pool.add(vec![pool.add(vec![a, b]), c])` produces `Add([a, b, c])`.
- **Normalization:** `pool.neg(pool.neg(x)) == x`, `pool.pow(x, pool.one) == x`.
- **Rational normalization:** `pool.rational(4, 6)` stores `Rational(2, 3)`.
- **String interning:** `pool.symbol("x") == pool.symbol("x")`.

### 6.2 Property-based tests (proptest)

- **Interning idempotence:** For any randomly generated `ExprNode`, interning it twice returns the same `ExprId`.
- **Hash-equality consistency:** If two nodes produce the same `ExprId`, they have the same content hash.
- **Commutativity:** `add([a, b]) == add([b, a])` for arbitrary `a`, `b`.
- **Associativity of flattening:** `add([add([a, b]), c]) == add([a, b, c])` for arbitrary `a`, `b`, `c`.
- **No-collision property:** For N randomly generated distinct expressions, all `ExprId`s are distinct.

### 6.3 Benchmarks (criterion)

- Intern 10,000 unique integer nodes.
- Intern 1,000 `Add` nodes of 10 children each.
- Look up (hit) 10,000 existing nodes.
- `map_bottom_up` over a 1,000-node DAG with identity transform.
- `contains_symbol` on a 1,000-node DAG.

Target: intern <200 ns/node, lookup <100 ns, map <1 ms for 1K nodes.

### 6.4 Fuzz testing (cargo-fuzz)

- Feed random byte sequences to the parser → expressions are interned → verify no panics and no invariant violations (sorted children, normalized rationals, no Neg(Neg)).
- Run for ≥1 hour before each release.

---

## 7. Action Items

1. [ ] Create `crates/monomix-kernel/src/expr.rs` with `ExprNode`, `ExprId`, `InternedStr`, `FnTag`
2. [ ] Implement `ExprPool` with interning, dedup map, and string table
3. [ ] Implement normalizing constructors (`add`, `mul`, `pow`, `neg`, `div`, `rational`)
4. [ ] Implement `map_bottom_up` and `fold` traversals
5. [ ] Implement `ExprHandle` and PyO3 `Expr` wrapper in `crates/monomix-py/`
6. [ ] Write unit tests for all invariants in §6.1
7. [ ] Write proptest suite for §6.2
8. [ ] Set up criterion benchmarks for §6.3
9. [ ] Set up cargo-fuzz target for §6.4
10. [ ] Benchmark and tune: confirm <200 ns/intern, <500 ns PyO3 overhead
