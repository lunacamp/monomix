# Monomix Rust Kernel — Implementation Spec

**Date:** 2026-05-07
**Scope:** Pure Rust kernel (`rust/monomix-kernel/`). No PyO3 bindings.
**References:** `SCOPE.md`, `designs/expression-dag.md`, `designs/parser.md`,
`designs/polynomial-ops.md`, `designs/simplifier.md`, `designs/differentiation.md`,
`designs/substitution.md`, `designs/numeric-eval.md`, `designs/equation-solving.md`

---

## 1. Crate layout

### 1.1 Workspace change

Add `rust/monomix-kernel` to the workspace root `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["rust/solver-bridge", "rust/monomix-kernel"]
```

### 1.2 New crate structure

```
rust/monomix-kernel/
├── Cargo.toml
└── src/
    ├── lib.rs            — re-exports public surface; pub use of all module tops
    ├── error.rs          — KernelError enum + all variants
    ├── expr/
    │   └── mod.rs        — ExprNode, ExprId, ExprPool, traversal helpers
    ├── parser/
    │   ├── mod.rs        — pub fn parse(), ParseResult, Stmt, Diagnostic
    │   ├── lexer.rs      — Token, TokenKind, Span, Lexer
    │   ├── ast.rs        — StmtKind, OutputMode, DiagnosticCode, SpanMap
    │   ├── expr.rs       — Pratt expression parser
    │   └── stmt.rs       — statement parser, error recovery
    ├── poly/
    │   └── mod.rs        — UnivPoly, view/to_expr, arithmetic, expand/collect/deg/coeff
    ├── simplify/
    │   ├── mod.rs        — pub fn simplify(), simplify_trig(), SimplifierConfig, SimplifyCache
    │   ├── driver.rs     — bottom-up traversal, fixed-point loop
    │   ├── numeric.rs    — exact constant folding
    │   ├── like_terms.rs — Add/Mul coefficient bucketing
    │   ├── powers.rs     — power consolidation
    │   ├── rational.rs   — Div orchestration over poly engine
    │   ├── patterns.rs   — Pattern, MetaVar, MatchEnv, Rule, RuleRegistry
    │   └── rules.rs      — built-in rules (Pythagorean in trig_rules(); DEFAULT_RULES empty)
    ├── diff/
    │   ├── mod.rs        — pub fn differentiate(), differentiate_fresh(), DiffCache
    │   ├── driver.rs     — recursive descent, memoization
    │   ├── arith.rs      — Mul/Div/Pow rules
    │   ├── functions.rs  — chain-rule plumbing
    │   ├── table.rs      — built-in derivative table (Sin..Atan)
    │   └── plugin.rs     — registration stub (no-op in Phase 1)
    ├── substitute/
    │   └── mod.rs        — substitute(), substitute_many(), substitute_fresh(), SubstituteCache
    ├── evalnum/
    │   └── mod.rs        — evaluate_numeric(), Bindings
    └── solve/
        └── mod.rs        — solve(), SolutionSet, linear/quadratic/Gauss
```

### 1.3 `Cargo.toml` dependencies

```toml
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
proptest   = "1"
criterion  = { version = "0.5", features = ["html_reports"] }
serde      = { version = "1", features = ["derive"] }
toml       = "0.8"

[[bench]]
name    = "kernel"
harness = false

[lints]
workspace = true
```

---

## 2. Error model

`KernelError` in `error.rs` is the single error type for the entire kernel.
All variants map to a named Python exception at the PyO3 boundary (future work).

```rust
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    // Parser
    #[error("parse error")] Parse(Vec<Diagnostic>),

    // Expression pool
    #[error("pool exhausted")] PoolExhausted,
    #[error("division by zero")] DivisionByZero { span: Option<Span> },
    #[error("indeterminate form 0/0")] IndeterminateForm,

    // Differentiator
    #[error("cannot differentiate an equation")] DifferentiateEquation,
    #[error("differentiation variable must be a symbol")] NotASymbol,

    // Substitution
    #[error("substitution target must be a symbol")] SubstituteNotASymbol,
    #[error("cyclic binding detected")] CyclicBinding,

    // Numeric evaluation
    #[error("unbound symbol: {0}")] UnboundSymbol(String),
    #[error("log of non-positive value")] LogOfNonPositive,
    #[error("sqrt of negative value")] SqrtOfNegative,
    #[error("domain error in {fn_name}")] DomainError { fn_name: &'static str },
    #[error("unsupported function for numeric eval")] UnsupportedFn,

    // Solver
    #[error("unsupported equation form: {reason}")] UnsupportedEquation { reason: String },
    #[error("singular system")] SingularSystem,

    // Arithmetic
    #[error("arithmetic overflow")] Overflow,
    #[error("numeric evaluation produced NaN")] NumericNaN,
}
```

---

## 3. Milestone 1 — Foundation

**Modules:** `expr`, `parser`, `poly`
**Deliverable:** `cargo test` green; `cargo bench` baseline established; cargo-fuzz target set up with legacy `.tst` seed corpus.

### 3.1 `expr` module

Implements `designs/expression-dag.md` in full.

Key commitments:
- `ExprId = LocalExprId(u32)` (type alias, migrates to `ContentExprId(u64)` in Phase 2 without call-site changes)
- `ExprNode` enum ≤ 32 bytes — enforced by `const _: [(); 32] = [(); std::mem::size_of::<ExprNode>()];`
- `ExprPool` with `Vec<ArenaEntry>` arena, identity-hashed FxHash dedup map, `IndexSet<String>` string table
- Pre-interned constants: `pool.zero`, `pool.one`, `pool.minus_one`; pre-interned common symbols `x, y, z, t, e, pi, i`
- Normalizing constructors: `add` (flatten+sort), `mul` (flatten+sort), `pow` (x^0→1, x^1→x), `neg` (neg(neg)→x), `div` (a/1→a), `rational` (GCD-normalize, q>0)
- `SmallInt(i64)` fast path; `BigInt(Box<BigInt>)` fallback; `pool.integer()` routes transparently
- `pool.subtree_size(id)` → `u32`, cached at intern time in `ArenaEntry`
- `map_bottom_up(pool, root, cache, f)` with caller-owned `FxHashMap<ExprId,ExprId>` + `map_bottom_up_fresh` convenience
- `fold(pool, root, init, f)` DAG fold (visited-set, not tree walk)
- `pool.contains_symbol(expr, sym)` via `fold`
- `pool.func_named(name: &str, args: Vec<ExprId>) -> ExprId` for constructing `Fn(Custom(...), ...)` nodes

Unit tests: interning roundtrip, structural uniqueness, flattening, normalization, rational normalization, string interning, `size_of` guard.
Proptest: interning idempotence, hash-equality consistency, commutativity (add/mul), no-collision on N random exprs.
Criterion: intern 10k integers, intern 1k Add-10 nodes, lookup hit 10k, `map_bottom_up` 1k-node identity, `contains_symbol` 1k-node.

### 3.2 `parser` module

Implements `designs/parser.md` in full.

Key commitments:
- Lexer decoupled from `ExprPool`: `Token::Ident(Span)` only; intern+lowercase at parser use-site
- `TokenKind` (Copy) for all Pratt binding-power dispatch — no `Token::clone()` in the inner loop
- `^` and `**` both lex to `Token::Pow`
- `inf`/`nan` float literals rejected at lex time with `DiagnosticCode::InvalidNumericLiteral`
- 1024-byte caps on identifiers and numeric literals
- Two-slot `ArrayVec<(Token, Span), 2>` lookahead buffer; `peek_at(1)` used only for `IDENT :=` detection
- `BuiltinTable` pre-interned at pool construction; dispatch via `InternedStr` integer equality
- `int` and `factor` parse normally but emit `UnsupportedStub` tagged nodes
- `SpanMap = FxHashMap<ExprId, Span>` side-table returned in `ParseResult`; arena nodes carry no spans
- `synchronise()` with paren-depth tracking for error recovery
- `DiagnosticCode::UnexpectedToken` carries `TokenKind`, not `String`

Unit tests: all token/span cases, SmallInt/BigInt boundary, precedence+associativity, multi-statement, assignment, built-in forms, error recovery.
Proptest: no-panics on arbitrary strings, span bounds, diagnostics non-overlapping.
Criterion: parse 100-term polynomial (<500µs), parse 1KB session (<200µs), lexer throughput (≥500K tokens/sec), pessimal-error input 1000 tokens (<2ms).
cargo-fuzz: seed corpus = all `legacy/reduce-algebra-code-r7357-trunk/packages/**/*.tst`; assert no panics; run ≥1h before v0.1.0.

### 3.3 `poly` module

Implements `designs/polynomial-ops.md` in full.

Key commitments:
- `UnivPoly = Vec<Term>` where `Term { exp: u32, coeff: ExprId }`, sorted descending by `exp`
- `view(pool, e, var)` recognizes univariate polynomial form; returns `Err` with structured reason for non-polynomial subterms
- `to_expr(pool, &poly)` re-emits through normalizing pool constructors
- `is_polynomial_in`, `common_univariate` predicates
- `poly_add`, `poly_sub`, `poly_mul` — linear merge / sparse convolution
- `poly_div(pool, f, g)` — schoolbook long division, returns `(quotient, remainder)`
- `expand(pool, expr)` distributes products and powers; `EXPAND_POW_LIMIT = 100` cap
- `collect(pool, expr, var)`, `deg(pool, expr, var)`, `coeff(pool, expr, var, n)`
- `poly_gcd` implemented but only invoked when `SimplifierConfig::gcd = true`

Unit tests: add/sub/mul/div on concrete polynomials, `view`/`to_expr` round-trip, `expand`/`collect`, GCD.
Proptest: `to_expr(view(e)) ≡ simplify(e)` for polynomial inputs; `deg(expand(e)) == deg(e)`.
Criterion: `expand((x+1)^20)` (<100ms), `poly_div` degree-10/degree-5 (<1ms).

### 3.4 Milestone 1 golden corpus infrastructure

Scaffold `tests/golden/` with:
- `tests/golden/README.md` describing the manifest format
- `tests/golden/divergences.toml` listing known intentional divergences with `reason` annotations
- `tests/golden/poly_div.toml` — ~15 curated pairs from `packages/poly/polydiv.tst` + `.rlg` (polynomial division expressions only; no procedure/for/on-off lines)
- `tests/golden/alg_expr.toml` — ~20 hand-curated pairs from `packages/alg/alg.tst` + `.rlg` (pure arithmetic/simplification statements only; `for`, `procedure`, `array`, `operator`, `on`/`off` lines excluded)
- A Rust integration test `tests/golden_tests.rs` that reads each manifest, calls `parse → operation → display`, and compares; divergence entries are `#[ignore]`-tagged with the reason in the ignore message

---

## 4. Milestone 2 — Operations

**Modules:** `simplify`, `diff`, `substitute`, `evalnum`, `solve`
**Deliverable:** `cargo test` green including all golden corpus tests; all SCOPE.md Phase 1 benchmark targets met; `cargo-fuzz` parse→simplify and parse→diff targets established.

### 4.1 `simplify` module

Implements `designs/simplifier.md` in full.

Key commitments:
- `simplify` entry point: bottom-up, ≤3 fixed-point iterations (`MAX_ITERS = 3`); debug build exposes `last_pass_count`; proptest asserts `≤ 2` for Phase 1 default rule set
- `simplify_trig` entry point: same driver, `trig_rules()` registry (Pythagorean only)
- `DEFAULT_RULES` is an empty `RuleRegistry` — default simplify applies NO trig identities (REDUCE-compatibility)
- `SimplifierConfig` defaults: `float_mode = Symbolic`, `gcd = false`, `expand_powers = false`, `mcd = false`
- `SimplifyCache`: `FxHashMap<ExprId, ExprId>`, full-clear eviction at 100k entries
- Like-terms bucket: hybrid — linear `SmallVec<[(ExprId, Coeff); 16]>` for ≤16 children, `FxHashMap` for larger
- `SmallVec` inline buffer sized at 32 for `fold_addends`/`fold_factors`
- `Coeff` enum: `Int(i64)` fast path, `Big(Box<BigInt>)`, `Rat(Box<Rational>)`
- `(x^a)^b` consolidation: conservative integer/rational guard table (see `designs/simplifier.md` §3.4)
- `simplify_div` delegates to `poly::poly_div`; raises `DivisionByZero` / `IndeterminateForm`

### 4.2 `diff` module

Implements `designs/differentiation.md` in full.

Key commitments:
- Per-call `DiffCache`, NOT session-scoped (result is var-dependent)
- n-ary Leibniz: pre-computes all `dchildren`, skips zero terms, emits exactly the non-zero Leibniz products
- 4-way `diff_pow` dispatch by `contains_symbol` on base and exponent
- Built-in table: Sin, Cos, Tan, Exp, Log, Sqrt, Asin, Acos, Atan; `Abs → None` (placeholder in Phase 1)
- `diff_fn` short-circuits on `du == pool.zero` before table lookup
- Unknown functions → `symbolic_df_placeholder` via `pool.func_named("df", [original, var])`
- `Eq` input → `KernelError::DifferentiateEquation`

### 4.3 `substitute` module

Implements `designs/substitution.md` in full.

Key commitments:
- `substitute(pool, cache, root, var, value)` — single-symbol bottom-up walk
- `substitute_many(pool, cache, root, bindings: &[(ExprId, ExprId)])` — parallel multi-binding (all replacements against original, one pass)
- `substitute_fresh` convenience wrapper
- `SubstituteCache = FxHashMap<ExprId, ExprId>` caller-owned
- Session binding resolver with cycle detection lives in the Python layer (not implemented here)
- `Eq(l, r)` is substituted componentwise: `Eq(sub(l), sub(r))`

### 4.4 `evalnum` module

Implements `designs/numeric-eval.md` in full.

Key commitments:
- `evaluate_numeric(pool, bindings, root) -> Result<f64, KernelError>`
- `Bindings = &[(ExprId, f64)]` consulted during walk; no pre-substitution
- NaN → `KernelError::NumericNaN`, never propagated as a float value
- All domain errors are typed: `LogOfNonPositive`, `SqrtOfNegative`, `DomainError { fn_name }`
- `Fn(Custom("df"), ...)` → `EvalError::UnsupportedFn` (not panics)

### 4.5 `solve` module

Implements `designs/equation-solving.md` in full.

Key commitments:
- Input: `Eq(lhs, rhs)` node; unknown(s) as `Symbol` ExprIds; result: `SolutionSet`
- `SolutionSet`: struct containing `solutions: Vec<Substitution>` (where `Substitution = Vec<(ExprId, ExprId)>`) and `has_complex_roots: bool`; empty `solutions` vec = no real solutions
- `has_complex_roots = true` signals the PyO3 layer to emit `MonomixWarning` (a Python-level concern; the kernel only sets the flag)
- Linear single: `a*x + b = 0` → `x = -b/a` (symbolic coefficients OK; extracted via `poly::view`)
- Quadratic single: formula; discriminant sign check; `solutions = []` + `has_complex_roots = true` for complex roots
- Linear system `n×n`: Gaussian elimination with partial pivoting; deterministic tie-breaker (smallest row index)
- `UnsupportedEquation { reason }` for cubic/quartic/transcendental forms
- All output ExprIds passed through `simplify` before return

### 4.6 Milestone 2 golden corpus additions

Complete and commit the curated golden manifests:
- `tests/golden/solve_linear_quadratic.toml` — ~10 pairs from `packages/solve/solve.tst` + `.rlg` (linear and simple quadratic `solve` calls only)
- `tests/golden/simplify.toml` — ~20 pairs from `packages/alg/alg.tst` covering simplification expressions
- `tests/golden/diff.toml` — 50-example differentiation corpus (from `packages/alg/alg.tst` `df` calls, cross-referenced with `.rlg`; per SCOPE.md §1.12 "curated 50-example textbook suite")

Divergence policy: every known divergence must have a `reason` entry in `divergences.toml` before the test is marked `#[ignore]`. Unnannotated failures are treated as regressions.

---

## 5. Testing summary

| Layer | Milestone 1 | Milestone 2 |
|-------|-------------|-------------|
| `cargo test` unit | expr, parser, poly | + simplify, diff, sub, evalnum, solve |
| `proptest` | DAG invariants, parser no-panics | + simplify idempotence, df linearity, Leibniz |
| `criterion` | pool intern, parser throughput, poly div | + simplify 50-term, df 20-term poly, solve quadratic |
| `cargo-fuzz` | parser (seed: all .tst files) | + parse→simplify, parse→diff |
| Golden corpus | poly_div (~15), alg_expr (~20) infra | + solve (~10), simplify (~20), diff (~50) |

---

## 6. SCOPE.md benchmark targets (Phase 1 success criteria)

All must pass before v0.1.0:

| Operation | Target |
|-----------|--------|
| `df` of a 20-term univariate polynomial | <50 ms wall-clock from Python (kernel: <20 ms) |
| `simplify` on a sum of 50 terms | <100 ms |
| `solve` on a quadratic | <10 ms |
| Pool intern per node | <200 ns |
| `map_bottom_up` on 1k-node DAG | <1 ms |

(Python-boundary targets deferred to the PyO3 milestone.)

---

## 7. Out of scope for this plan

- PyO3 bindings (`crates/monomix-py/`)
- Python `Session` object and REPL
- Plugin registration API (contract defined in `patterns.rs`/`plugin.rs`, entry point deferred to Phase 2)
- `simplify_trig` trig rules beyond Pythagorean
- Phase 2 subsystems (integration, matrix ops, polynomial factorization, multivariate poly, Gaussian elimination beyond `n×n` linear systems)
- `cargo-fuzz` runs >1h (required before v0.1.0 release, not during development)
