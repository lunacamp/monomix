# Substitution — System Design

**Component:** `monomix-kernel::substitute`
**Status:** Design phase
**Date:** 2026-05-04 (split from `designs/substitution-numeric-eval.md`, originally dated 2026-05-03)
**References:** SCOPE.md §1.8, §1.3, §0.4, §0.7; ADR-0001; ADR-0002; `designs/expression-dag.md`; `designs/parser.md`; `designs/simplifier.md`; `designs/polynomial-ops.md`; `designs/equation-solving.md`; `designs/numeric-eval.md`

---

## 1. Requirements

### 1.1 Functional requirements

The `substitute` engine replaces every occurrence of one or more symbols inside an expression with a replacement expression, returning a new `ExprId`. The walk is *parallel*: when multiple bindings are supplied, all replacements happen on a single tree pass against the *original* subterms, never against the result of a sibling substitution.

The required surface from SCOPE.md §1.8:

- `sub(x = 5, expr)` (REDUCE-style, parsed by `designs/parser.md` §3.3) and `Expr.subs({x: 5})` (Pythonic alternative). Both lower to `substitute::substitute_many` with a single-element binding list for the simple case.
- `Session.resolve(expr)` for the REPL's "evaluate symbols against the current binding table" path (SCOPE.md §1.3, §1.9).

The substitution engine is consumed by:

- The **solver's matrix builder** for the "constant column" computation (`designs/equation-solving.md` §3.6.2 — `substitute::all_to_zero`).
- The **REPL** when displaying a bound variable's resolved value (`Session.evaluate(...)`, SCOPE.md §1.9).
- The **MCP server** (Phase 1.5) for the `subs` tool endpoint.
- The **parser builtin** `sub(x = 5, expr)` (`designs/parser.md` §3.3).
- The **numeric evaluator** indirectly, via `Session.resolve(...)` materialising bindings before `evaluate_numeric` is called (`designs/numeric-eval.md` §1.1).
- Plugin-defined functions that need to substitute inside their arguments (Phase 1.10).

### 1.2 Non-functional requirements

- **Bounded time.** `substitute` is a single bottom-up walk over the input DAG, linear in the number of distinct nodes. There is no fixed-point loop and no recursive simplifier callback.
- **No panics.** Errors are returned as `KernelError::SubstituteError` variants. They surface in Python as `monomix.SubstitutionError` (per SCOPE.md §0.4).
- **Determinism.** Two `substitute` calls with identical inputs and binding order produce structurally identical `ExprId` outputs.
- **DAG-safe.** The walk uses the visited-set discipline from `designs/expression-dag.md` §3.6. A subexpression shared between two parents is visited once, not twice. This is what makes substituting into deeply-shared expressions (the simplifier's typical output) tractable.
- **Cycle-safe at the Session boundary.** Bindings can in principle form cycles (`x := y; y := x;`). The Session's binding resolver detects them and raises `CyclicBinding` rather than recursing forever. The substitution engine itself does not see the binding cycle — the Session resolves to a fixed point first.
- **Preserves `Eq` structure.** `substitute(pool, Eq(l, r), x, v)` returns `Eq(substitute(l, x, v), substitute(r, x, v))`. The solver depends on this for iterative system solving (`designs/equation-solving.md` §3.6.4).

### 1.3 Constraints

- **Symbol-target only.** Phase 1's `substitute` accepts `(Symbol, ExprId)` bindings. Pattern-based substitution (`sub(f(_) = 1, ...)`) and rule-based substitution (REDUCE's `let` operator) are Phase 2 deliverables (SCOPE.md §2.6 — General Pattern Matching). The simplifier's pattern-matching engine (`designs/simplifier.md` §3.6) is the right home for those when they land.
- **No lazy evaluation.** SCOPE.md §1.8 explicit cut: "No lazy evaluation or delayed substitution." Every `substitute` call materialises its result immediately. Symbolic closures, lazy promises, and call-by-need are out of scope for the entire MVP.
- **Bindings are shallow.** When a binding's RHS itself contains a bound symbol, the Session's resolver expands one level at a time with cycle detection (§3.6); the substitute engine does not auto-recurse on the result. This matches REDUCE's `let`-rule discipline and avoids infinite loops on `x := f(x)`-shaped self-reference.

### 1.4 What this component is **not**

To pin scope precisely:

- It is **not the parser.** The parser handles `sub(x = 5, expr)` syntax and lowers it (`designs/parser.md` §3.3). The engine receives an already-parsed `ExprId` plus a binding map.
- It is **not the simplifier.** It neither rewrites nor cancels; it replaces. A `substitute` result is not in normal form unless the caller follows up with `simplify` (`designs/simplifier.md` §2.1).
- It is **not the Session.** Bindings live on `Session` (Python side, SCOPE.md §1.3). The substitute engine is passed an explicit binding map; the Session is the policy layer that decides which bindings to pass.
- It is **not the numeric evaluator.** `evaluate_numeric` consumes a `Bindings` view directly and does not need a pre-substituted expression — the two engines compose but neither subsumes the other (`designs/numeric-eval.md` §4.1).

---

## 2. High-Level Design

### 2.1 Public API

```rust
//! crates/monomix-kernel/src/substitute/mod.rs

/// Replace every occurrence of `var` (a Symbol ExprId) inside `root` with `value`,
/// returning a new ExprId. The walk is bottom-up and DAG-aware: shared subterms are
/// visited once, and the result graph is re-interned through pool constructors.
///
/// `cache` is a caller-owned scratch buffer keyed on input ExprId. Long pipelines
/// (substitute → simplify → substitute → ...) can reuse the same allocation; one-
/// shot callers should use `substitute_fresh`.
///
/// Errors:
/// - `KernelError::SubstituteError(NotASymbol(id))` — `var` is not a Symbol.
pub fn substitute(
    pool: &mut ExprPool,
    cache: &mut SubstituteCache,
    root: ExprId,
    var: ExprId,
    value: ExprId,
) -> Result<ExprId, KernelError>;

/// Multi-variable parallel substitution. All replacements are computed against the
/// original subterms — substituting `{x: y, y: x}` into `x + y` yields `y + x`, not
/// `x + x` (sequential) or `y + y` (sequential the other way).
///
/// Errors:
/// - `KernelError::SubstituteError(NotASymbol(id))` — any binding key is not a Symbol.
/// - `KernelError::SubstituteError(DuplicateKey(sym))` — `bindings` contains the same
///   symbol twice with conflicting values.
pub fn substitute_many(
    pool: &mut ExprPool,
    cache: &mut SubstituteCache,
    root: ExprId,
    bindings: &[(ExprId, ExprId)],
) -> Result<ExprId, KernelError>;

/// Convenience wrappers — allocate a fresh cache per call.
pub fn substitute_fresh(pool: &mut ExprPool, root: ExprId, var: ExprId, value: ExprId)
    -> Result<ExprId, KernelError>;
pub fn substitute_many_fresh(pool: &mut ExprPool, root: ExprId, bindings: &[(ExprId, ExprId)])
    -> Result<ExprId, KernelError>;

/// Per-call memoization scratch. Wraps a `FxHashMap<ExprId, ExprId>` keyed on the
/// pre-substitution ExprId; cleared by the caller between calls only when the binding
/// set has changed (the cache is binding-scoped — see §3.2).
pub struct SubstituteCache { /* FxHashMap<ExprId, ExprId> + binding-set fingerprint */ }

impl SubstituteCache {
    pub fn new() -> Self;
    pub fn clear(&mut self);
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum SubstituteError {
    #[error("substitute target {0:?} is not a Symbol")]
    NotASymbol(ExprId),
    #[error("duplicate binding key {0:?}")]
    DuplicateKey(Symbol),
}
```

`SubstituteError` is flattened into `KernelError::SubstituteError(_)` at the boundary so callers pattern-match without threading through an extra `Result`. This matches the convention from `designs/equation-solving.md` §3.9 and `designs/simplifier.md` §3.9.

### 2.2 Component diagram

```
                  ExprId (root)
                       │
                       ▼
            ┌─────────────────────┐
            │ substitute/         │
            │   engine.rs         │
            │  (map_bottom_up,    │
            │   binding lookup)   │
            └──────────┬──────────┘
                       │
                       │ replace if Symbol matches
                       │ otherwise re-intern children
                       │
                       ▼
            ┌──────────────────────┐
            │ pool.add / .mul /    │
            │ .pow / .div / .neg / │
            │ .eq / .func / .list  │
            │ (re-interned)        │
            └──────────┬───────────┘
                       │
                       ▼
                  new ExprId
```

The mirror pipeline for numeric evaluation is in `designs/numeric-eval.md` §2.2. Both walks consume `bindings` shaped data, but the substitute walk writes back into the pool whereas evalnum produces a scalar.

### 2.3 Module layout

```
crates/monomix-kernel/src/substitute/
├── mod.rs              — public API, SubstituteError, re-exports
├── engine.rs           — single & multi substitution; bottom-up walk over the DAG
├── cache.rs            — SubstituteCache (FxHashMap + binding-set fingerprint)
└── tests.rs
```

The shared `map_bottom_up` primitive lives in `expression-dag` (`designs/expression-dag.md` §3.6) and is consumed here. The numeric-eval engine sits in a sibling `evalnum/` directory and does not cross-import this module — the operations have nothing in common at the implementation level (substitute writes to the pool, evalnum doesn't).

### 2.4 Algorithm choices at a glance

| Operation | Algorithm | Complexity | Notes |
|-----------|-----------|------------|-------|
| `substitute` (single var) | `map_bottom_up` with cache; rewrite `Symbol(s) → value` if `s == var`, otherwise re-intern children unchanged | O(distinct nodes in `root`) — DAG, not tree | Cache reuse across pipeline calls makes repeated subs into the same root cheap |
| `substitute_many` | Same walk; binding lookup is `FxHashMap<Symbol, ExprId>` | O(distinct nodes) + O(1) per Symbol | Parallel: input subterms are looked up against the *original* binding map, never against substituted siblings |
| Binding resolution (Session-side) | One-level `substitute_many` with `currently_resolving` set for cycle detection | O(depth of binding chain) per resolve | Detects `x := y; y := x;` in O(2) — see §3.6 |

### 2.5 Parallel vs. sequential substitution — the contract

`substitute_many` is **parallel**, in the standard CAS sense: substituting `{x: y, y: x}` into `x + y` yields `y + x`, not `x + x` or `y + y`. This matches REDUCE's `sub` operator ([sub.red:.../packages/alg/sub.red](../legacy/reduce-algebra-code-r7357-trunk/packages/alg/sub.red)) and Mathematica's `ReplaceAll`. It is the only sane default for swap-substitutions and for "rename one variable" use cases.

The sequential alternative — apply binding 1, then binding 2, then ... — is available by calling `substitute` repeatedly. Pipeline composition (`substitute → simplify → substitute`) is the typical sequential pattern and the substitute cache supports it across calls (§3.2).

The parser (`designs/parser.md` §3.3) lowers `sub(x=a, y=b, expr)` to a single `substitute_many(expr, [(x, a), (y, b)])` call — confirmed parallel by the explicit ordering of the parser's lowering, despite the parser comment loosely calling it "nested substitutions" (it means nested in the parse tree, not nested in execution).

### 2.6 Single-pass, no fixed-point

`substitute` has no fixed-point loop. Each call performs:

1. Validate that every binding key is a `Symbol`.
2. Build a `FxHashMap<Symbol, ExprId>` from the binding list (rejecting duplicates).
3. `map_bottom_up`: for each unique node, either replace it (if it is a matched Symbol) or re-intern its children unchanged. Cached at every node.
4. Return the rewritten root.

Substitute followed by simplify (the typical pipeline) is the user's responsibility, not a contract of the engine. This contrasts with the simplifier's bottom-up rewrite (`designs/simplifier.md` §2.4) which *is* iterative because rewrite rules can expose new rewrite opportunities — substitution's rewrite is one-shot.

---

## 3. Deep Dive

### 3.1 Single substitution (`substitute/engine.rs`)

```rust
pub fn substitute(
    pool: &mut ExprPool,
    cache: &mut SubstituteCache,
    root: ExprId,
    var: ExprId,
    value: ExprId,
) -> Result<ExprId, KernelError> {
    let var_sym = match pool.get(var) {
        ExprNode::Symbol(s) => Symbol(*s),
        _ => return Err(KernelError::Substitute(SubstituteError::NotASymbol(var))),
    };
    cache.bind(BindingFingerprint::single(var_sym, value));
    let mut work = SingleWork { pool, cache, var: var_sym, value };
    Ok(work.go(root))
}

struct SingleWork<'a> {
    pool: &'a mut ExprPool,
    cache: &'a mut SubstituteCache,
    var: Symbol,
    value: ExprId,
}

impl<'a> SingleWork<'a> {
    fn go(&mut self, id: ExprId) -> ExprId {
        if let Some(&hit) = self.cache.get(id) { return hit; }

        let new_id = match self.pool.get(id).clone() {
            // Atoms — only Symbol can match.
            ExprNode::Symbol(s) if Symbol(s) == self.var => self.value,
            ExprNode::Symbol(_)
            | ExprNode::SmallInt(_)
            | ExprNode::BigInt(_)
            | ExprNode::Rational(_)
            | ExprNode::Float(_)
            | ExprNode::String(_) => id,

            // Composites — recurse on children, re-intern with same constructor.
            ExprNode::Add(children) => {
                let new = self.map_children(&children);
                self.pool.add(new)
            }
            ExprNode::Mul(children) => {
                let new = self.map_children(&children);
                self.pool.mul(new)
            }
            ExprNode::Pow(b, e)  => { let nb = self.go(b); let ne = self.go(e); self.pool.pow(nb, ne) }
            ExprNode::Neg(x)     => { let nx = self.go(x); self.pool.neg(nx) }
            ExprNode::Div(n, d)  => { let nn = self.go(n); let nd = self.go(d); self.pool.div(nn, nd) }
            ExprNode::Eq(l, r)   => { let nl = self.go(l); let nr = self.go(r); self.pool.eq(nl, nr) }
            ExprNode::Fn(tag, args) => {
                let new = self.map_children(&args);
                self.pool.func(tag, new)
            }
            ExprNode::List(items) => {
                let new = self.map_children(&items);
                self.pool.list(new)
            }
        };
        self.cache.insert(id, new_id);
        new_id
    }

    fn map_children(&mut self, kids: &[ExprId]) -> Vec<ExprId> {
        kids.iter().map(|&c| self.go(c)).collect()
    }
}
```

**Re-interning preserves normalisation.** The pool's constructors (`pool.add`, `pool.mul`, `pool.pow`, etc., per `designs/expression-dag.md` §3.5) flatten, sort, and fold trivial cases (`x^0 → 1`, `x^1 → x`, `neg(neg(x)) → x`). Substituting `x` with `y` in `Mul([x, x])` produces `Mul([y, y])` which the pool normalises to `Pow(y, 2)` through its eager rules. This is *not* simplification (the simplifier does much more); it is the structural normalisation that the pool guarantees on every constructor call.

**`map_children` allocates a `Vec`.** A previous iteration of this design used a `SmallVec<[ExprId; 8]>` to mirror `designs/polynomial-ops.md` §3.1's inline-buffer discipline. The choice was reverted because `pool.add` and `pool.mul` already take `Vec<ExprId>` (per `designs/expression-dag.md` §3.5), and converting between SmallVec and Vec at the boundary erased the savings. The intermediate Vec is dropped immediately when the child loop returns, so its lifetime is one stack frame.

**No cache miss on shared subterms.** The cache is the load-bearing optimisation. A deep DAG with many shared subexpressions (the typical simplifier output) has many fewer distinct nodes than a tree representation would, and the cache visits each distinct node exactly once. Without the cache, substituting into the simplifier's output of an expanded `(x+1)^10` (which is a sum-of-products with significant sharing) would take O(2^10) instead of O(11).

### 3.2 Multi-variable substitution (`substitute/engine.rs`)

```rust
pub fn substitute_many(
    pool: &mut ExprPool,
    cache: &mut SubstituteCache,
    root: ExprId,
    bindings: &[(ExprId, ExprId)],
) -> Result<ExprId, KernelError> {
    if bindings.is_empty() { return Ok(root); }
    if bindings.len() == 1 {
        let (k, v) = bindings[0];
        return substitute(pool, cache, root, k, v);
    }

    let mut map: FxHashMap<Symbol, ExprId> = FxHashMap::default();
    for &(k, v) in bindings {
        let sym = match pool.get(k) {
            ExprNode::Symbol(s) => Symbol(*s),
            _ => return Err(KernelError::Substitute(SubstituteError::NotASymbol(k))),
        };
        match map.entry(sym) {
            Entry::Occupied(e) if *e.get() != v => {
                return Err(KernelError::Substitute(SubstituteError::DuplicateKey(sym)));
            }
            Entry::Occupied(_) => { /* identical re-binding — silently dedup */ }
            Entry::Vacant(e)   => { e.insert(v); }
        }
    }
    cache.bind(BindingFingerprint::many(&map));
    let mut work = MultiWork { pool, cache, map: &map };
    Ok(work.go(root))
}
```

`MultiWork::go` is identical to `SingleWork::go` except the Symbol match arm consults the map instead of comparing against a single variable:

```rust
ExprNode::Symbol(s) => {
    match self.map.get(&Symbol(s)) {
        Some(&value) => value,
        None         => id,
    }
}
```

**Why dedup silently for identical re-binding.** A common Python idiom is `expr.subs({x: 5, **defaults})` where `defaults` may include `x: 5` redundantly. Raising on identical-value re-binding would force every caller to defensively merge dicts; allowing it is a small ergonomics win with no semantic ambiguity. Mismatched values (`{x: 5, x: 6}`) are the actual error case.

**Binding-set fingerprint on the cache.** The cache is invalidated when the binding set changes — substituting `x → y` and then substituting `x → z` against the same root cannot reuse cache entries. The fingerprint is a 64-bit hash of `(Vec<(Symbol, ExprId)> sorted by Symbol)`; the `cache.bind(...)` call clears the cache if the fingerprint differs from the previous call. This means a typical pipeline that calls `substitute` once per pass with different bindings does not benefit from the cache across passes — but the *single* deeply-shared pass within a call does.

For the pipeline pattern where the same binding set is applied repeatedly (the solver's back-substitution chains values forward — `designs/equation-solving.md` §3.6.4 — though at most once per row), the cache pays off across calls.

### 3.3 The Eq-substitution contract — a worked example

`substitute(pool, cache, Eq(x + 1, 2*x), x, 5)` walks:

```
                    Eq                                       Eq
                   /  \                                     /  \
                  /    \                                   /    \
                Add    Mul          (substitute x=5)     Add    Mul
               /  \   /  \                              /  \   /  \
              x   1  2   x                             5   1  2   5
                                                          ↓ pool.add re-interns
                                                       SmallInt(6)         SmallInt(10)
                                                              \   /
                                                               Eq
                                                              /  \
                                                          SmallInt(6)  SmallInt(10)
```

The pool's `add` constructor folds `[5, 1]` to `SmallInt(6)` and `mul` folds `[2, 5]` to `SmallInt(10)` because both are eager numeric reductions in the pool (`designs/expression-dag.md` §3.5). The result `Eq(6, 10)` is structurally `false` but the substitution engine does not say so — the result is a perfectly valid `Eq` expression, and a downstream caller (the solver, or a `simplify_eq` rule in Phase 2) is responsible for the semantic interpretation.

This is the contract `designs/equation-solving.md` §3.6.4 depends on for its back-substitution check: `is_definitely_zero(pool, m[i][n])` after each substitution relies on the pool having already folded `Add([0, 0])` to `pool.zero` rather than keeping the structural form.

### 3.4 The convenience wrapper for the solver

`designs/equation-solving.md` §3.6.2 calls a thin wrapper `substitute::all_to_zero(pool, expr, vars)` that substitutes every variable in `vars` to `pool.zero` simultaneously. It is implemented as:

```rust
pub fn all_to_zero(pool: &mut ExprPool, expr: ExprId, vars: &[ExprId]) -> Result<ExprId, KernelError> {
    let mut cache = SubstituteCache::new();
    let bindings: Vec<(ExprId, ExprId)> = vars.iter().map(|&v| (v, pool.zero)).collect();
    substitute_many(pool, &mut cache, expr, &bindings)
}
```

It is exported from `mod.rs` so the solver's matrix builder doesn't have to construct the binding list inline. The wrapper is in this module, not in the solver, because the binding-list shape is the substitute engine's contract.

### 3.5 Substitution from the Session's perspective

The Session's responsibilities (Python-side, SCOPE.md §1.3) layered on top of the substitute engine:

1. **Maintaining the binding table.** A `dict[str, Expr]` keyed by user-visible name. `:=` assignments update it; `clear x` removes an entry.
2. **Resolving references at read time.** When the user types `y` and `y := x + 1`, the Session must call `substitute_many` to resolve `y → x + 1`, then recursively resolve `x` if `x` is also bound, with cycle detection (§3.6).
3. **Choosing which bindings to pass to evalnum.** When the user calls `evaluate_numeric(expr)`, the Session passes its full binding table to `designs/numeric-eval.md`'s engine; when they call `expr.subs({x: 5})` then `.evaluate_numeric()`, the Session passes only the explicit dict.

The substitute engine itself does *not* know about the Session — every call is self-contained, with the Session deciding the binding set per-call. This is the same discipline the simplifier uses (`designs/simplifier.md` §2.1 — `SimplifierConfig` is caller-supplied) and it matches REDUCE's `let`/`clear` rule discipline where rules are always explicit at the call site, never implicit from "the current shell state".

### 3.6 Cycle detection in the Session resolver

The substitute engine cannot detect binding cycles because it sees only one expression at a time. The Session resolver does. This logic lives on the Python side because the Session owns the binding table and the cycle-detection state; the kernel just receives the resolved binding list per call.

```python
class Session:
    def __init__(self, pool):
        self._pool = pool
        self._bindings: dict[str, ExprId] = {}
        self._cycle_cap = 256  # per-resolve depth limit, defensive backstop

    def resolve(self, root: ExprId) -> ExprId:
        """Expand bindings inside `root` to a fixed point with cycle detection."""
        seen: set[Symbol] = set()
        return self._resolve_inner(root, seen, depth=0)

    def _resolve_inner(self, root: ExprId, seen: set[Symbol], depth: int) -> ExprId:
        if depth > self._cycle_cap:
            raise CyclicBinding(f"binding chain exceeded {self._cycle_cap} levels")

        # One pass of substitute_many over the current bindings.
        free = self._free_symbols(root)
        active = [(sym, val) for sym, val in self._bindings.items() if sym in free and sym not in seen]
        if not active:
            return root  # fixed point reached

        # Detect cycle: any of the active values references back into `seen`.
        for sym, val in active:
            seen.add(sym)
            if self._references_any(val, seen):
                raise CyclicBinding(f"cycle through {sym}")

        substituted = monomix_kernel.substitute_many(self._pool, root, active)
        # Recurse on the result — the substituted RHSs may themselves contain bindings.
        return self._resolve_inner(substituted, seen, depth + 1)
```

**Why one-pass-then-recurse instead of "substitute until stable".** A binding chain `a := b; b := c; c := 5` resolves in 3 steps regardless of order. The recursive implementation is `O(chain length)` deep and `O(distinct free symbols)` wide. A single `substitute_many` over all bindings simultaneously would not work because the result of substituting `a → b` then needs `b → c`, and parallel substitution does not chain.

**The `seen` set carries forward.** Once `a` has been resolved, its symbol cannot appear in the recursion — adding `a` to `seen` and excluding bindings keyed on `seen` symbols breaks the would-be infinite loop on `a := a + 1`. This is the contract the numeric evaluator depends on for `evaluate_numeric` over its result substitutions (`designs/numeric-eval.md` §3.5).

**The depth cap is a defensive backstop.** Healthy resolution chains are short (under 10 deep is typical); the 256 cap protects against pathological inputs the cycle detector somehow misses (or against deliberately-deep `a₀ := a₁; a₁ := a₂; …` chains that aren't cyclic but are memory hostile).

### 3.7 PyO3 boundary

The substitution engine is exposed to Python via `Session.subs(...)` and `Expr.subs(dict)`.

```rust
#[pyfunction]
fn subs(py: Python<'_>, session: &PySession, expr: &PyExpr, bindings: &PyDict)
    -> PyResult<PyExpr>
{
    let pool_handle = expr.pool.clone();
    // Convert the Python dict to the kernel's binding list.
    let raw_bindings = py_dict_to_bindings(py, &pool_handle, bindings)?;
    let id = expr.id;
    let subtree_size = pool_handle.read().subtree_size(id);
    let new_id = if subtree_size > 500 {
        py.allow_threads(|| {
            let mut pool  = pool_handle.write();
            let mut cache = SubstituteCache::new();
            monomix_kernel::substitute::substitute_many(&mut pool, &mut cache, id, &raw_bindings)
        })?
    } else {
        let mut pool  = pool_handle.write();
        let mut cache = SubstituteCache::new();
        monomix_kernel::substitute::substitute_many(&mut pool, &mut cache, id, &raw_bindings)?
    };
    Ok(PyExpr { pool: pool_handle, id: new_id })
}
```

**GIL release threshold.** Same `subtree_size > 500` cutoff as the simplifier (`designs/simplifier.md` §3.8), polynomial engine (`designs/polynomial-ops.md` §3.8), and solver (`designs/equation-solving.md` §3.8). Uniform policy across all kernel surface ops keeps the boundary-overhead profile predictable and avoids one-off analysis per function.

**Write lock.** `substitute` allocates new `ExprId`s through pool constructors, so the boundary takes a *write* lock on the pool. This is unlike `evaluate_numeric` (`designs/numeric-eval.md` §3.6), which only reads.

### 3.8 Error handling

| Error | Source | Handling |
|-------|--------|----------|
| `SubstituteError::NotASymbol(id)` | `pool.get(var)` is not Symbol | Return; user error or parser bug |
| `SubstituteError::DuplicateKey(sym)` | `substitute_many` sees the same Symbol twice with different values | Return; identical-value duplicates are silently deduplicated |
| `CyclicBinding` (Python-only) | Session resolver detects cycle | Raised at the resolver, never enters the kernel |

The engine never panics. Internal invariant violations (e.g. `ExprPool::children` returning the wrong arity for an Add node) are caught by `debug_assert!` in debug builds and become benign no-ops in release builds — `substitute` returns the input unchanged on assertion failure and the boundary surfaces a generic `KernelError::Internal` mapped to a "internal error, please report" message.

---

## 4. Trade-off Analysis

### 4.1 Bottom-up rewrite vs. top-down pattern match

**Chosen: bottom-up rewrite via `map_bottom_up`, with cache.**

The alternative is a top-down rewriter that pattern-matches at each level and recursively descends only into children that haven't been replaced.

| Approach | Allocations on shared subtrees | Cache discipline | Code complexity |
|----------|-------------------------------|------------------|-----------------|
| Bottom-up + cache (chosen) | One per distinct node | Visited-set keyed on ExprId | Low — same shape as simplifier driver |
| Top-down pattern-match | Up to one per *occurrence* (without cache) | Harder to memoize because rewrite depends on context | Medium — must handle each ExprNode variant inline |

The bottom-up path is uniform and reuses the `map_bottom_up` primitive defined in `designs/expression-dag.md` §3.6 — every kernel transformation that walks a DAG uses this primitive (substitute, simplify, differentiate). Adding a top-down variant specifically for substitute would be a one-off pattern that diverges from the rest of the kernel. The cache effectiveness is the load-bearing argument: the simplifier's output has heavy DAG sharing, and the cache turns substitution's complexity from "tree size" to "DAG size".

### 4.2 Substitute then simplify vs. simplify-during-substitute

**Chosen: substitute returns un-simplified output; caller invokes `simplify` separately.**

A natural alternative is to fold `simplify` into the substitute walk — every re-interned node also goes through the simplifier's normalization. This is what Mathematica's `ReplaceAll[expr, rules]` does (sort of), and what some Lisp-based CAS systems do.

| Approach | Allocations per substitute | Output canonicality | Composition cost |
|----------|---------------------------|---------------------|------------------|
| Substitute only (chosen) | Pool-normalised but not simplified | Pool's eager rules: yes; simplifier rules: no | One `simplify` call after the substitute — explicit |
| Substitute + simplify (rejected) | Twice as many: each interned node also routes through simplifier | Fully canonical | Implicit; user can't opt out |

The reason against folding is that substitute is sometimes a stepping stone: the solver's matrix builder substitutes `var → 0` once per (eq, var) pair to compute the constant column (`designs/equation-solving.md` §3.6.2), then substitutes more values later. Forcing a simplify pass on every intermediate would balloon the elimination path's cost. Letting the caller decide when to `simplify` is the right separation.

The pool's eager normalization (`designs/expression-dag.md` §3.5) does the small wins for free: substituting `x → 0` and re-interning `Add([0, y])` produces just `y` because the pool drops zero terms in `add`. That covers the common cases without invoking the simplifier's full machinery.

### 4.3 Parallel vs. sequential `substitute_many` semantics

**Chosen: parallel.** REDUCE, Mathematica, SymPy, Maxima — every CAS that does substitution defaults to parallel substitution. The contract is the only sane one for swap-substitutions like `{x: y, y: x}`, and the only one that matches user expectation on `subs({a: 1, b: 2})` (the user does not want `b: 2` to override an earlier substitution that happens to introduce `a`).

The sequential alternative (apply each binding in order) is occasionally useful for controlled rewrites; it is available by chained single-`substitute` calls.

### 4.4 Architectural divergence from REDUCE — separate `subs` vs. `let`-rules

**Chosen: `substitute_many` as a one-shot operation, distinct from any rule database.**

REDUCE's `let` operator installs a *persistent rewrite rule* that fires every time the simplifier encounters a matching subterm. It is the substrate for both substitution-style replacement and pattern-based rewriting.

| Property | REDUCE `let`-rules | Monomix `substitute` (chosen) |
|----------|-------------------|-------------------------------|
| Persistence | Persistent until `clearrules` | One-shot per call |
| Pattern matching | Full pattern matcher (Phase 2 in Monomix) | Symbol-only |
| Composition with simplify | Rules fire during simplify | Caller invokes substitute then simplify explicitly |
| User mental model | "I taught the simplifier a fact" | "I replaced these symbols just here" |
| Implementation surface | Tied to the simplifier's rule database | Standalone module |

Phase 1 picks the simpler one-shot model. Phase 2's pattern matcher (`designs/simplifier.md` §3.6) is the home for `let`-style persistent rules — when that lands, the substitute engine remains as the ergonomic shortcut for the "replace this symbol just here" use case. The two coexist; one is not a strict generalization of the other because their persistence semantics differ.

The Python boundary's `Expr.subs({...})` and `Session.let(rule)` (Phase 2) are deliberately distinct method names to keep the user's mental model clean.

---

## 5. Scale, Limits, and Future Work

### 5.1 Phase 2: Pattern-based substitution (SCOPE.md §2.6)

The Phase 1 substitute is symbol-only. Phase 2's general pattern matcher (`designs/simplifier.md` §3.6) extends the surface to:

- `substitute(expr, f(_), 1)` — replace every `f(anything)` with `1`.
- `substitute(expr, x^_, 0)` — replace every power of x with 0.
- Conditional patterns: `substitute(expr, x => x > 0, x^2)`.

The Phase 2 path is:

1. Add a `Pattern` enum (`Symbol`, `Wildcard`, `Application`, `Conditional`, …) in a new `kernel/src/pattern/` module.
2. Generalize `substitute_many`'s binding key from `Symbol` to `Pattern`.
3. Reuse the Phase 1 walk: each visited node attempts to match against each pattern in the binding list; first match wins (per REDUCE's first-fit semantics).

Estimated effort: ~4 weeks (the matcher itself is the substantive work; integrating with substitute is straightforward).

### 5.2 Phase 2: `let`-style persistent rules

Once pattern matching exists, persistent rules become a natural extension. The `Session` would carry a `rule_database: Vec<Pattern, ExprId>` alongside the binding table, and `simplify` would consult it before each top-down rewrite pass. Phase 2 deliverable per `designs/simplifier.md` §5.1.

### 5.3 Phase 2: Lazy evaluation / delayed substitution

SCOPE.md §1.8 explicitly defers this. A future `Lazy(thunk)` ExprNode variant could hold an unevaluated computation that fires on first read; this is what Mathematica-style `:>` (RuleDelayed) and `Hold[]` constructs implement. The substitute engine would gain a "force lazy values" flag and the corresponding evalnum extension is tracked in `designs/numeric-eval.md` §5. Out of Phase 1 scope.

### 5.4 Performance characteristics

For Phase 1, expected complexity:

| Input | Time | Allocations |
|-------|------|-------------|
| `substitute(x → 5, e)`, `e` is small (< 100 nodes) | O(distinct nodes) ~ 1 ms | Cache HashMap + intermediate Vec per composite |
| `substitute_many({x → 5, y → 7}, e)`, `e` is large (10k nodes) | O(distinct nodes) ~ 10 ms | Cache HashMap + per-call binding hashmap |
| Session resolve (`x := y; y := 5; resolve(x + y)`) | O(chain depth × tree size) | Per-recursion substitute cache |

The dominant term is the cache-HashMap lookups. The §6.3 benchmarks pin this — the "DAG-shared big substitute" benchmark is the regression target for the cache; without the cache, the same input would be O(2^depth) in the substitute walk.

---

## 6. Testing Strategy

### 6.1 Unit tests (`cargo test`)

**Single variable, atom replacement:**

- `substitute(x, x, 5)` ⟹ `5`.
- `substitute(y, x, 5)` ⟹ `y` (no match).
- `substitute(5, x, 99)` ⟹ `5` (no Symbol nodes).
- `substitute(x + 1, x, 5)` ⟹ `6` (pool's eager add normalises `5 + 1`).
- `substitute(x * 2 + 3, x, 5)` ⟹ `13`.

**Single variable, expression replacement:**

- `substitute(x^2, x, y + 1)` ⟹ `(y + 1)^2` (pool's eager pow does *not* expand).
- `substitute(x + y, x, y)` ⟹ `2*y` (pool's eager add coalesces like terms).
- `substitute(sin(x), x, 0)` ⟹ `sin(0)` (the simplifier, not substitute, would fold to 0).
- `substitute(Eq(x, 5), x, 7)` ⟹ `Eq(7, 5)` (preserves Eq structure; no semantic check).

**Multi-variable (parallel):**

- `substitute_many(x + y, [(x, y), (y, x)])` ⟹ `y + x` (which interns to `x + y` — pool sorts; structurally equal to original).
- `substitute_many({x: 1, y: 2}, x + y)` ⟹ `3`.
- `substitute_many({x: y, y: 1}, x)` ⟹ `y` (parallel — does NOT chain to `1`).
- `substitute_many({x: y, y: 1}, x + y)` ⟹ `y + 1`.
- `substitute_many({x: 5, x: 5}, x + 1)` ⟹ `6` (silent dedup of identical re-binding).
- `substitute_many({x: 5, x: 7}, x + 1)` ⟹ `Err(DuplicateKey(x))`.

**Error paths:**

- `substitute(e, 5, 7)` ⟹ `Err(NotASymbol(5))`.
- `substitute(e, sin(x), 0)` ⟹ `Err(NotASymbol(sin(x)))` (sin is a Fn, not a Symbol).

**DAG sharing:**

- A heavily shared expression `e = (x + 1) * (x + 1)` (one Add node referenced twice from a Mul) substitutes `x → y` in O(2) work, not O(4). Verified by counting cache lookups in a debug build.

**Session resolve — cycles:**

- `Session({x: y, y: 5}).resolve(x)` ⟹ `5` (one-step chain).
- `Session({x: y, y: x}).resolve(x)` ⟹ `Err(CyclicBinding)`.
- `Session({x: x}).resolve(x)` ⟹ `Err(CyclicBinding)` (self-reference).
- `Session({x: y, y: z, z: w, …}).resolve(x)` for chain length 100 ⟹ resolves to the terminal value within `cycle_cap`.

### 6.2 Property-based tests (`proptest`)

- **Idempotence on disjoint vars:** for random `e`, random `(x, v)`, `substitute(substitute(e, x, v), x, v) == substitute(e, x, v)` (the first sub removes all `x`s; the second is a no-op).
- **Commutativity for disjoint binding sets:** for random `e` and disjoint bindings `B1, B2`, `substitute_many(substitute_many(e, B1), B2) == substitute_many(e, B1 ∪ B2)`.
- **Parallel-substitution swap correctness:** for random `e` containing both `x` and `y`, `substitute_many(e, [(x, y), (y, x)])` swaps the symbols. Verified by checking that every `Symbol(x)` in the result is `Symbol(y)` and vice versa.
- **Preserves DAG structure:** for random `e` with deliberate sharing (constructed via `pool.add([sub, sub])` where sub appears twice), the substituted result has the same sharing depth (counted via `subtree_size`).
- **Eval ∘ substitute equivalence (cross-doc):** for random `e` with all symbols bound to numeric values, `evalnum(substitute_many(e, bindings), {}) == evalnum(e, bindings)` (within tolerance for f64 ops). This invariant is jointly owned with `designs/numeric-eval.md` §6.2.

### 6.3 Benchmarks (`criterion`)

| Benchmark | Target |
|-----------|--------|
| `substitute(x → 5, small_expr)` (≤ 10 nodes) | <100 µs |
| `substitute_many({x → 1, y → 2, z → 3}, expr)` (≤ 100 nodes) | <500 µs |
| `substitute(x → big_expr, big_expr_with_many_xs)` (≥ 1000 nodes; DAG-shared) | <10 ms |
| `Session.resolve(deeply_chained_binding)` (chain length 50) | <5 ms |

The "DAG-shared big substitute" benchmark is the regression target for the cache — without the cache, the same input would be O(2^depth) in the substitute walk. If it regresses past target, the cache is broken or being invalidated unnecessarily.

### 6.4 Fuzz testing (`cargo-fuzz`)

- **Target:** `substitute(parse(arbitrary_bytes), x, parse(arbitrary_bytes))`. Asserts (a) no panics, (b) the output's `subtree_size` is bounded by a multiple of input size + binding-value size (no exponential blow-up from re-interning), (c) every Symbol(s) in the output that equals `x` came from the binding value (no leftover references).
- **Target:** `substitute_many(e, swap_bindings({x, y, z}))` — random triplet swaps. Asserts the parallel-swap correctness invariant from §6.2 holds for random inputs.
- **Seed corpus:** the legacy `.tst` files (`legacy/reduce-algebra-code-r7357-trunk/packages/alg/sub.tst` and substitution-exercising files in `packages/poly/`) plus pathological inputs (deeply nested Add/Mul with many shared subterms).
- **Run duration:** ≥1 hour per release (combined with the parser, simplifier, polynomial, evalnum, and solver fuzz targets).

### 6.5 Golden-corpus tests (`pytest`)

A subset of `legacy/reduce-algebra-code-r7357-trunk/packages/alg/sub.{tst,rlg}`. For each `.tst` input, parse, run `subs`, render result, and compare against `.rlg`.

**Known intentional divergences from REDUCE** (recorded in the manifest with `# reason: ...` annotations):

- **`subs` is parallel by default; REDUCE's `sub` is also parallel but `let` rules are applied iteratively.** Phase 1 has no `let` — substitute is the only mechanism, and it is parallel. Documented case-by-case where REDUCE's `let`-based test would produce different output than our parallel `subs`.
- **No automatic re-substitution after sub.** REDUCE re-runs the simplifier (with `let` rules) after every sub, which can fire further substitutions transitively. Phase 1 requires the user to invoke `simplify` explicitly. Tests are annotated where REDUCE's transitive expansion produces a different shape.

The curated set lives in `tests/golden/sub/` with the manifest mapping input file to expected output and the `# reason: ...` annotation per case.

---

## 7. Action Items

### Phase 1 — Core implementation

1. [ ] Create `crates/monomix-kernel/src/substitute/mod.rs` exposing the public API (§2.1); wire `SubstituteError` into `KernelError` with `NotASymbol`, `DuplicateKey` variants
2. [ ] Implement `substitute/engine.rs` — bottom-up walk with `SingleWork` and `MultiWork`, ExprNode dispatch, re-interning through pool constructors (§3.1, §3.2)
3. [ ] Implement `substitute/cache.rs` — `SubstituteCache` with binding-set fingerprint invalidation (§3.2)
4. [ ] Export `substitute::all_to_zero` convenience wrapper (§3.4) for the solver's matrix builder
5. [ ] Wire `substitute_many` into the Python `Session` via PyO3 with the `subtree_size > 500` GIL-release threshold and a write-lock on the pool (§3.7)
6. [ ] Implement Python-side `Session.resolve(root)` with cycle detection and `_cycle_cap = 256` defensive depth limit (§3.6)
7. [ ] Coordinate with `designs/parser.md` §3.3 to confirm the `sub(x = a, y = b, expr)` lowering produces a single `substitute_many` call with the bindings in argument order — not a chain of singles
8. [ ] Coordinate with `designs/equation-solving.md` §3.6.2 on the `substitute::all_to_zero` integration with the matrix builder
9. [ ] Add `pool.subtree_size(id)` if the DAG design hasn't yet (§3.7 depends on it; already an action item in `designs/simplifier.md` §3.8 — confirm shared)

### Phase 1 — Verification

10. [ ] Unit-test all transformations enumerated in §6.1, including the structured-error paths and the DAG-sharing case
11. [ ] `proptest` substitute idempotence + commutativity + parallel-swap (§6.2). The cross-doc evalnum-substitute equivalence test is shared with `designs/numeric-eval.md` §6.2
12. [ ] `criterion` benchmarks including the DAG-shared substitute regression guard (§6.3)
13. [ ] `cargo-fuzz` targets for substitute and the parallel-swap invariant (§6.4)
14. [ ] Curate the golden-corpus `.tst`/`.rlg` subset for substitution, with a divergence manifest covering the intentional divergences in §6.5
15. [ ] Confirm SCOPE.md §1.8 invariants hold: parallel substitution semantics, no lazy evaluation

### Phase 2 — Generalization (deferred)

16. [ ] Add `Pattern` enum + integrate into `substitute_many`'s binding key for pattern-based substitution (§5.1)
17. [ ] Implement `let`-style persistent rules on `Session` consulting the simplifier's rule database (§5.2)
18. [ ] Add `Lazy(thunk)` ExprNode variant + a `force` flag in substitute for delayed substitution (§5.3)
