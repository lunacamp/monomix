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
/// Copy, Eq, Hash — 4 bytes in Phase 1. This is what every kernel function passes around.
///
/// Defined as a type alias over an opaque `LocalExprId(u32)` so that the Phase 2
/// migration to content-addressed identity (a 64-bit truncated content hash that
/// is meaningful across machines — see §5.4) can be a single change to this alias.
/// Pool API and call sites do not change.
pub type ExprId = LocalExprId;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalExprId(u32);

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
///
/// Variant sizing rules (see §3.4 for rationale):
/// - Large payloads (BigInt, Rational pair) are boxed so the enum stays compact.
/// - Composite children use `Box<[ExprId]>` (immutable exact-sized slice) instead of
///   `Vec<ExprId>` — saves 8 bytes (no capacity field) and signals immutability.
/// - `SmallInt(i64)` is the fast path for integer literals; only fall through to
///   `BigInt` when the value exceeds 64 bits. ~99% of literals fit in i64.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum ExprNode {
    // --- Atoms ---
    SmallInt(i64),                       // fast path: fits in 64 bits
    BigInt(Box<num_bigint::BigInt>),     // arbitrary precision fallback
    Rational(Box<(BigInt, BigInt)>),     // boxed pair; always in lowest terms, q > 0
    Float(OrderedFloat<f64>),            // OrderedFloat for Eq + Hash
    Symbol(InternedStr),
    String(InternedStr),

    // --- Composite (children are immutable Box<[ExprId]>) ---
    Add(Box<[ExprId]>),                  // n-ary, flattened, sorted
    Mul(Box<[ExprId]>),                  // n-ary, flattened, sorted
    Pow(ExprId, ExprId),                 // base, exponent
    Neg(ExprId),                         // unary minus
    Div(ExprId, ExprId),                 // numerator, denominator
    Eq(ExprId, ExprId),                  // equation lhs = rhs
    Fn(FnTag, Box<[ExprId]>),            // function application
    List(Box<[ExprId]>),                 // ordered collection
}
```

### 2.2 ExprPool — the interning table

```rust
pub struct ExprPool {
    /// Arena of all expression nodes paired with their cached content hash.
    /// ExprId indexes into this. Caching the hash avoids re-walking child arrays
    /// on every dedup lookup — see §3.2.
    nodes: Vec<ArenaEntry>,

    /// Deduplication map: content hash → ExprId.
    /// Keyed by raw u64 with an identity hasher (BuildHasherDefault<IdentityHasher>)
    /// so the hashmap does not re-hash an already-good hash.
    dedup: HashMap<u64, SmallVec<[ExprId; 1]>, BuildHasherDefault<IdentityHasher>>,

    /// String interning table: string content → InternedStr.
    strings: IndexSet<String>,

    /// Pre-interned common constants for fast access.
    pub zero: ExprId,
    pub one: ExprId,
    pub minus_one: ExprId,
}

/// One slot in the arena. The cached hash means dedup lookups never re-walk a node.
struct ArenaEntry {
    hash: u64,
    node: ExprNode,
}
```

The pool is the single owner of all expression data. Every kernel function takes `&ExprPool` (read) or `&mut ExprPool` (create nodes). The pool is never cloned — it's wrapped in `Arc<RwLock<ExprPool>>` at the PyO3 boundary so that traversal-heavy operations (`get`, `children`, `fold`, `contains_symbol`) can proceed concurrently while only `intern()` requires an exclusive lock. See §3.7 and §5.2 for the concurrency rationale.

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
        ExprNode::SmallInt(n) => n.hash(&mut h),
        ExprNode::BigInt(n) => n.hash(&mut h),
        ExprNode::Symbol(s) => s.hash(&mut h),
        ExprNode::Add(children) => {
            // Children are sorted ExprIds — hash in order
            for c in children.iter() { c.hash(&mut h); }
        }
        // ... etc for each variant
    }
    h.finish()
}
```

**Hash caching.** The hash is computed once when a node is first interned and stored in the `ArenaEntry` alongside the node. Subsequent `intern()` calls hash only the *candidate* node; collision-list comparisons read the cached hash directly — there is no rehashing of nodes already in the arena. For wide n-ary nodes (`Add` with hundreds of children — common in polynomial expansion) this turns dedup lookup from O(fan-out) per candidate into O(1) per candidate.

**Identity-hasher for the dedup map.** The dedup map key is already a 64-bit FxHash. Wrapping the map with `BuildHasherDefault<IdentityHasher>` (a no-op hasher that returns the key unchanged) prevents the standard library's `HashMap` from re-hashing an already-good hash — a measurable win on the hot path.

Hash collisions are handled by the `SmallVec<[ExprId; 1]>` in the dedup map — on collision, linear scan with structural equality. In practice, `FxHasher` on small integer sequences produces very few collisions.

### 3.3 String interning

Symbol and string literal text is stored once in an `IndexSet<String>`. `InternedStr(u32)` is the index. Benefits:

- Symbol equality is integer comparison.
- No string allocation after the first occurrence.
- `IndexSet` preserves insertion order, which gives deterministic iteration if needed.

Common symbols (`x`, `y`, `z`, `t`, `e`, `pi`, `i`) are pre-interned at pool construction for fast access.

### 3.4 Memory layout

Target: ≤32 bytes per arena slot (8-byte cached hash + ≤24-byte enum). Every slot in `Vec<ArenaEntry>` is sized to `max(variant)` because of Rust enum layout, so keeping the *largest* variant small is what matters — not the median.

**Variant sizing after boxing large payloads:**

```
ExprNode::SmallInt(i64)                  —  16 B  (discriminant + i64)
ExprNode::BigInt(Box<BigInt>)            —  16 B  (discriminant + ptr; BigInt on heap)
ExprNode::Rational(Box<(BigInt,BigInt)>) —  16 B  (discriminant + ptr; pair on heap)
ExprNode::Float(OrderedFloat<f64>)       —  16 B  (discriminant + f64)
ExprNode::Symbol(InternedStr)            —   8 B  (discriminant + u32)
ExprNode::String(InternedStr)            —   8 B
ExprNode::Add(Box<[ExprId]>)             —  24 B  (discriminant + ptr + len)
ExprNode::Mul(Box<[ExprId]>)             —  24 B
ExprNode::Pow(ExprId, ExprId)            —  16 B  (discriminant + 2× u32)
ExprNode::Neg(ExprId)                    —   8 B
ExprNode::Div(ExprId, ExprId)            —  16 B
ExprNode::Eq(ExprId, ExprId)             —  16 B
ExprNode::Fn(FnTag, Box<[ExprId]>)       —  32 B  (FnTag is enum w/ Custom payload)
ExprNode::List(Box<[ExprId]>)            —  24 B
```

`ExprNode` itself is therefore ≤32 bytes (sized to `Fn`, the largest variant). Adding the cached `u64` hash brings each `ArenaEntry` to ~40 bytes. A 10K-node expression occupies ~400 KB — fits comfortably in L2 cache on modern CPUs.

**Why `Box<[ExprId]>` instead of `Vec<ExprId>`:** children of `Add`/`Mul`/`Fn`/`List` are immutable after interning. `Box<[T]>` stores only `(ptr, len)` — 16 bytes — vs. `Vec<T>` which stores `(ptr, len, capacity)` — 24 bytes. Saves 8 bytes per composite node and signals immutability.

**Why `SmallInt(i64)` separately from `BigInt`:** `num_bigint::BigInt` always heap-allocates its digit storage. The vast majority of integer literals in CAS workloads (coefficients, exponents, small constants) fit in `i64` — covering them with an inline variant eliminates a heap round-trip per node. The pool's `integer()` constructor selects the variant transparently.

The `nodes: Vec<ArenaEntry>` arena gives dense, cache-friendly storage. Traversal is a sequence of index lookups into a contiguous array — no pointer chasing for the node enum itself, though `BigInt` and composite children are one pointer hop away.

**Regression guard:** `const _: [(); 32] = [(); std::mem::size_of::<ExprNode>()];` in the test module fails compilation if a future variant inflates the enum.

### 3.5 Constructor API

The pool exposes typed constructors that enforce invariants at creation time. Callers never construct `ExprNode` directly.

```rust
impl ExprPool {
    // --- Atoms ---
    pub fn integer(&mut self, n: impl Into<BigInt>) -> ExprId;   // routes to SmallInt or BigInt
    pub fn small_int(&mut self, n: i64) -> ExprId;               // direct i64 fast path
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
/// Memoized via a caller-owned cache so multi-pass pipelines (e.g. simplify ∘
/// substitute ∘ differentiate) can reuse the same allocation across passes
/// instead of dropping and re-creating a HashMap on every call.
pub fn map_bottom_up(
    pool: &mut ExprPool,
    root: ExprId,
    cache: &mut FxHashMap<ExprId, ExprId>,
    f: &mut dyn FnMut(&mut ExprPool, ExprId) -> ExprId,
) -> ExprId {
    map_impl(pool, root, f, cache)
}

/// Convenience wrapper for one-shot callers that don't care about cache reuse.
pub fn map_bottom_up_fresh(
    pool: &mut ExprPool,
    root: ExprId,
    f: &mut dyn FnMut(&mut ExprPool, ExprId) -> ExprId,
) -> ExprId {
    let mut cache = FxHashMap::default();
    map_bottom_up(pool, root, &mut cache, f)
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
    pool: Arc<RwLock<ExprPool>>,
    id: ExprId,
}
```

Python method calls on `Expr` acquire the appropriate lock, perform the operation, and return a new `ExprHandle`. Read-only operations (`get`, `children`, `fold`, `contains_symbol`, structural equality) take a read lock — multiple Python threads (e.g. concurrent MCP requests) can traverse the pool simultaneously. Only `intern()` and the normalizing constructors take a write lock. The lock is held only during the operation — not across Python statements. The GIL is released before acquiring the lock for operations >1 ms.

**Why `RwLock` and not `Mutex`:** typical CAS workloads are read-heavy after an initial construction phase. Differentiation of a 10K-node expression performs ~10K read traversals interspersed with ~5K interning writes. A `Mutex` would serialize all of these; an `RwLock` lets the read traversals proceed in parallel across threads. This is also the lock type that scales gracefully into Phase 1.5 (MCP server) without further changes.

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

**High-water mark instrumentation.** The pool exposes `len()` and emits a structured log warning when node count crosses configurable thresholds (default: warn at 1M nodes, error at 10M). This gives Phase 1.5 sizing data without committing to a GC implementation in Phase 1.

For long-running MCP server sessions (Phase 1.5+), pool growth may become an issue. Options for future phases:

- **Generational pool:** partition nodes into generations; collect old generations when no live `ExprId` references them. Requires tracking which `ExprId`s are reachable from Python.
- **Session-scoped pools:** each MCP request gets its own pool; results are copied into a shared output pool. Avoids garbage collection entirely.
- **Copy-on-access from a frozen pool:** freeze the pool after each request; next request works on a fresh pool that can look up (but not modify) the frozen one. Avoids full copies.

This is explicitly deferred — Phase 1 ships without GC, and the design revisit happens before Phase 1.5 (MCP) based on measured pool sizes.

### 5.2 Parallel simplification

Phase 1 already uses `RwLock<ExprPool>` (§3.7), giving concurrent traversal across threads with exclusive interning. This is sufficient for Phase 1 and Phase 1.5 (read-heavy MCP request handling). For Phase 2+, finer-grained concurrency within a single simplification pass is desirable. Options:

- **Per-thread local pools with merge:** each thread interns locally; results are merged into the shared pool at join. Avoids contention entirely but requires a merge step that re-interns produced subtrees in the parent pool. Compatible with the read/write phase split below.
- **Lock-free arena:** append-only arena with atomic length counter. Reads are safe without locking; writes use `compare_exchange` on the length. The dedup map is the real bottleneck — requires a concurrent hashmap (`dashmap` or `flurry`).
- **Read-phase / write-phase split for the simplifier:** Phase 1 (parallel, read-only): walk the DAG to compute the set of transformations to apply, keyed by `ExprId`. Phase 2 (sequential, exclusive): apply them in one pass. Fits naturally with the existing `fold` primitive and avoids the lock-free-dedup complexity.

The read/write phase split is the recommended Phase 2 path because it composes with `RwLock` rather than replacing it.

### 5.3 Serialization

For Phase 2 (result caching, script loading), expressions need serialization. The arena-based design makes this natural: serialize the `nodes` vec and `strings` set; `ExprId` values are stable indices *within* the serialized pool. A compact binary format (e.g., `postcard` or `bincode`) can serialize/deserialize a pool in a single pass.

For cross-machine transport, see §5.4 — pool-local indices are not portable.

### 5.4 Multi-machine / distributed processing

The Phase 1 `ExprId(u32)` is a **pool-local index** — `ExprId(42)` on machine A and `ExprId(42)` on machine B refer to entirely different nodes. This is a deliberate trade for single-machine speed but is a barrier to distributed simplification. Phase 2 introduces content-addressed identity:

```rust
// Phase 2: content-addressed ID — same content produces the same ID anywhere.
pub struct ContentExprId(u64);  // truncated FxHash / Blake3 of the canonical encoding
pub type ExprId = ContentExprId;
```

With content addressing:

- **Cross-machine deduplication is automatic.** Two workers independently constructing `x^2 + 1` arrive at the same `ExprId` without coordination. No reconciliation step is required when results are merged.
- **Result caching is durable across machines.** A cached `simplify(ExprId(0xdead…))→ExprId(0xbeef…)` mapping is valid on any worker that shares the same canonical encoding. Backed by Redis, S3, or a distributed KV store.
- **Work distribution is natural.** A coordinator splits a large expression at root children, ships subexpressions (their `ExprId`s plus the transitive `ExprId→ExprNode` mapping) to workers, and reassembles the result via `pool.add([...])`. Shared subexpressions are transferred and computed exactly once.

**Why this is a one-line migration.** `ExprId` is defined in §2.1 as a type alias over `LocalExprId(u32)`. Switching to `ContentExprId(u64)` changes only:
1. The alias.
2. `ExprPool::intern()` — replaces "push to `Vec`, return index" with "compute content hash, insert into `HashMap<ContentExprId, ArenaEntry>`".
3. The arena type — `Vec<ArenaEntry>` becomes `HashMap<ContentExprId, ArenaEntry>`.

All call sites — parser, simplifier, differentiator, PyO3 boundary — remain unchanged. They pass an opaque `ExprId` and never depend on its representation.

**Trade-off.** 64-bit IDs vs. 32-bit IDs double handle size and slightly worsen cache density (8 bytes vs. 4 bytes per child slot in `Box<[ExprId]>`). Equality remains O(1). The distribution capability is transformative for Phase 2 workloads (large-scale algebraic computation, parallel solver, MCP cluster).

**64-bit collision resistance.** A 64-bit truncated hash supports ~10^9 distinct nodes before birthday-collision probability exceeds 1%. For pool sizes up to ~10^7 nodes, collision probability is negligible (<10^-5). For larger workloads, the alternative is 128-bit truncation or full Blake3.

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

### Phase 1 — Core implementation
1. [ ] Create `crates/monomix-kernel/src/expr.rs` with `ExprNode`, `ExprId` (alias over `LocalExprId`), `InternedStr`, `FnTag`
2. [ ] Define `ExprNode` with boxed large variants (`BigInt`, `Rational`) and `Box<[ExprId]>` children for `Add`/`Mul`/`Fn`/`List`
3. [ ] Add `SmallInt(i64)` fast-path variant; route `pool.integer()` through it for values fitting in `i64`
4. [ ] Implement `ExprPool` with `Vec<ArenaEntry>` arena (cached hash + node), identity-hashed dedup map, and string table
5. [ ] Implement normalizing constructors (`add`, `mul`, `pow`, `neg`, `div`, `rational`)
6. [ ] Implement `map_bottom_up` (caller-owned cache) + `map_bottom_up_fresh` and `fold` traversals
7. [ ] Implement `ExprHandle` over `Arc<RwLock<ExprPool>>` and PyO3 `Expr` wrapper in `crates/monomix-py/`
8. [ ] Add pool high-water-mark instrumentation (warn at 1M nodes, error at configurable limit)

### Phase 1 — Verification
9. [ ] Write unit tests for all invariants in §6.1
10. [ ] Write proptest suite for §6.2
11. [ ] Set up criterion benchmarks for §6.3
12. [ ] Set up cargo-fuzz target for §6.4
13. [ ] Add compile-time `size_of::<ExprNode>()` regression guard (target ≤32 bytes)
14. [ ] Benchmark and tune: confirm <200 ns/intern, <500 ns PyO3 overhead, ≥40 bytes median `ArenaEntry`

### Phase 2 — Scalability follow-ups (deferred)
15. [ ] Implement read-phase / write-phase split in the simplifier for parallel within-request simplification (§5.2)
16. [ ] Migrate `ExprId` alias from `LocalExprId(u32)` to `ContentExprId(u64)`; replace `Vec<ArenaEntry>` arena with content-addressed `HashMap` (§5.4)
17. [ ] Add cross-machine result cache (Redis or KV store) keyed on `ContentExprId`
18. [ ] Evaluate generational pool / session-scoped pool for Phase 1.5+ MCP workloads (§5.1)
