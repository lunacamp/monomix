//! Golden-corpus regression tests.
//!
//! Each manifest under `tests/golden/` is a TOML file of `(input, expected,
//! ignore?, ignore_reason?)` entries. The runner is **manifest-typed**: each
//! manifest is exercised by the operation it claims to test, and the
//! `expected` field is checked structurally where feasible.
//!
//! Verification strategy per manifest:
//!
//! - **alg_expr.toml, simplify.toml** — `simplify(input)` must canonicalize
//!   to the same `ExprId` as `simplify(expected)`. Interning makes `ExprId`
//!   equality a sound structural check; we don't need a display layer.
//!
//! - **diff.toml** — the input is a `df(target, var)` call. We extract its
//!   arguments, call `differentiate`, simplify, and compare against the
//!   simplified `expected`.
//!
//! - **poly_div.toml** — every entry must be polynomial in `x`
//!   (the manifest's primary variable). Verified via `is_polynomial_in`.
//!
//! - **solve_linear_quadratic.toml** — `expected` is descriptive English
//!   ("parseable linear", "parseable quadratic …"). The runner extracts the
//!   form claim and asserts `deg(input, x)` matches.
//!
//! When `expected` cannot be parsed in our Phase 1 subset (e.g. solver output
//! `{x = 3}`), the runner falls back to a parse-only smoke check for that
//! entry and prints a `SMOKE:` line so the partial coverage is visible. A
//! manifest summary line at the end records how many entries were verified
//! versus smoke-checked, which makes it easy to spot regressions in coverage.
//!
//! `ignore = true` entries continue to be skipped, with `ignore_reason`
//! printed for traceability.

use monomix_kernel::diff::differentiate;
use monomix_kernel::expr::{ExprId, ExprNode, ExprPool, FnTag};
use monomix_kernel::parser::parse;
use monomix_kernel::poly::{deg, is_polynomial_in};
use monomix_kernel::simplify::{simplify, SimplifierConfig, SimplifyCache};
use serde::Deserialize;

/// One golden-corpus row.
///
/// `ignore_reason` is documentation only — the runner just prints it next to
/// the `SKIP:` line. Two categories of reason are acceptable; see
/// `tests/golden/README.md` for the full convention:
///
/// 1. **Unimplemented feature** (free prose), e.g. "df() result display not
///    yet implemented". Most current ignores. Flip to `ignore = false` when
///    the feature lands.
/// 2. **Intentional REDUCE divergence**, referencing a divergence id from
///    `tests/golden/divergences.toml`, e.g. "symbol-ordering: REDUCE sorts
///    alphabetically". Rare.
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

#[derive(Default)]
struct Tally {
    verified: usize,
    smoke: usize,
    skipped: usize,
}

impl Tally {
    fn print_summary(&self, manifest: &str) {
        println!(
            "manifest {}: {} verified, {} parse-smoke (expected unparseable), {} skipped",
            manifest, self.verified, self.smoke, self.skipped
        );
    }

    /// Enforce a soft per-manifest floor on `VERIFIED` count. Catches the
    /// rot-mode where new entries get added as `ignore = true` (or
    /// downgraded to `SMOKE:` parse-only) faster than verified ones land —
    /// silently shrinking real regression coverage even as the manifest
    /// grows. The floor is conservative (matches the existing baseline);
    /// raise it when a manifest's verified count climbs durably.
    fn assert_min_verified(&self, manifest: &str, min_verified: usize) {
        assert!(
            self.verified >= min_verified,
            "manifest {} has only {} VERIFIED entries (floor {}); the corpus \
             is rotting. Either re-verify some {} skipped / {} smoke entries, \
             or — if a regression in feature support is the cause — lower \
             the floor with an explicit rationale.",
            manifest, self.verified, min_verified, self.skipped, self.smoke
        );
    }
}

/// Parse a single-statement source. Appends a terminator if missing. Returns
/// `None` if parsing failed or yielded no statements (e.g., `expected` uses
/// syntax outside the Phase 1 subset).
fn try_parse_one(pool: &mut ExprPool, src: &str) -> Option<ExprId> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return None;
    }
    let with_term = if trimmed.ends_with(';') || trimmed.ends_with('$') {
        trimmed.to_string()
    } else {
        format!("{};", trimmed)
    };
    let result = parse(&with_term, pool);
    if !result.diagnostics.is_empty() || result.statements.is_empty() {
        return None;
    }
    Some(result.statements[0].expr)
}

fn parse_input_or_panic(pool: &mut ExprPool, entry: &Entry) -> ExprId {
    try_parse_one(pool, &entry.input).unwrap_or_else(|| {
        panic!("Parse error for input {:?}", entry.input);
    })
}

fn simplify_id(pool: &mut ExprPool, id: ExprId) -> ExprId {
    let cfg = SimplifierConfig::default();
    let mut cache = SimplifyCache::new();
    simplify(pool, id, &cfg, &mut cache)
}

/// Runner for `alg_expr.toml` and `simplify.toml`: simplify(input) and
/// simplify(expected) must produce the same `ExprId`. Falls back to parse-
/// smoke when `expected` doesn't parse in the Phase 1 subset.
fn run_simplify_match(path: &str, min_verified: usize) {
    let manifest = load_manifest(path);
    let mut t = Tally::default();
    for entry in &manifest.entries {
        if entry.ignore {
            t.skipped += 1;
            println!("SKIP [{}]: {}", entry.ignore_reason, entry.input);
            continue;
        }
        let mut pool = ExprPool::new();
        let in_id = parse_input_or_panic(&mut pool, entry);
        let in_simp = simplify_id(&mut pool, in_id);

        match try_parse_one(&mut pool, &entry.expected) {
            Some(exp_id) => {
                let exp_simp = simplify_id(&mut pool, exp_id);
                assert_eq!(
                    in_simp, exp_simp,
                    "simplify({:?}) = {:?}; simplify({:?}) = {:?}; \
                     these should be the same canonical ExprId",
                    entry.input,
                    pool.get(in_simp),
                    entry.expected,
                    pool.get(exp_simp),
                );
                t.verified += 1;
                println!("VERIFY: {}  ≡  {}", entry.input, entry.expected);
            }
            None => {
                t.smoke += 1;
                println!(
                    "SMOKE: {} (expected {:?} not parseable in Phase 1 subset)",
                    entry.input, entry.expected
                );
            }
        }
    }
    t.print_summary(path);
    t.assert_min_verified(path, min_verified);
}

/// Runner for `diff.toml`: each input is a `df(target, var)` call.
/// Extract the args, run `differentiate`, simplify, compare with simplified
/// `expected`. Inputs that aren't `df(...)` calls fall through to smoke.
fn run_diff_match(path: &str, min_verified: usize) {
    let manifest = load_manifest(path);
    let mut t = Tally::default();
    for entry in &manifest.entries {
        if entry.ignore {
            t.skipped += 1;
            println!("SKIP [{}]: {}", entry.ignore_reason, entry.input);
            continue;
        }
        let mut pool = ExprPool::new();
        let in_id = parse_input_or_panic(&mut pool, entry);

        let (target, var) = match pool.get(in_id).clone() {
            ExprNode::Fn(FnTag::Custom(name), args)
                if pool.str_of(name) == "df" && args.len() == 2 =>
            {
                (args[0], args[1])
            }
            _ => {
                t.smoke += 1;
                println!("SMOKE: {} (input is not a df() call)", entry.input);
                continue;
            }
        };

        let derived = differentiate(&mut pool, target, var)
            .unwrap_or_else(|e| panic!("differentiate({}) failed: {:?}", entry.input, e));
        let derived_simp = simplify_id(&mut pool, derived);

        match try_parse_one(&mut pool, &entry.expected) {
            Some(exp_id) => {
                let exp_simp = simplify_id(&mut pool, exp_id);
                assert_eq!(
                    derived_simp, exp_simp,
                    "diff({:?}) simplified to {:?}; expected ({:?}) simplified to {:?}",
                    entry.input,
                    pool.get(derived_simp),
                    entry.expected,
                    pool.get(exp_simp),
                );
                t.verified += 1;
                println!("VERIFY: {}  →  {}", entry.input, entry.expected);
            }
            None => {
                t.smoke += 1;
                println!(
                    "SMOKE: {} (expected {:?} not parseable in Phase 1 subset)",
                    entry.input, entry.expected
                );
            }
        }
    }
    t.print_summary(path);
    t.assert_min_verified(path, min_verified);
}

/// Runner for `poly_div.toml`: every non-ignored entry must be polynomial in
/// `x`. `expected` is informational (typically the same as input).
fn run_poly_check(path: &str, min_verified: usize) {
    let manifest = load_manifest(path);
    let mut t = Tally::default();
    for entry in &manifest.entries {
        if entry.ignore {
            t.skipped += 1;
            println!("SKIP [{}]: {}", entry.ignore_reason, entry.input);
            continue;
        }
        let mut pool = ExprPool::new();
        let in_id = parse_input_or_panic(&mut pool, entry);
        let x = pool.symbol("x");
        assert!(
            is_polynomial_in(&mut pool, in_id, x),
            "{:?} is not polynomial in x; this manifest requires every entry \
             to be a polynomial in the primary variable",
            entry.input
        );
        t.verified += 1;
        println!("POLY-OK: {}", entry.input);
    }
    t.print_summary(path);
    t.assert_min_verified(path, min_verified);
}

/// Runner for `solve_linear_quadratic.toml`: `expected` is descriptive
/// English ("parseable linear", "parseable quadratic ..."). Match on the
/// keyword and assert `deg(input, x)` is consistent.
fn run_solve_form(path: &str, min_verified: usize) {
    let manifest = load_manifest(path);
    let mut t = Tally::default();
    for entry in &manifest.entries {
        if entry.ignore {
            t.skipped += 1;
            println!("SKIP [{}]: {}", entry.ignore_reason, entry.input);
            continue;
        }
        let mut pool = ExprPool::new();
        let in_id = parse_input_or_panic(&mut pool, entry);
        let x = pool.symbol("x");
        let lower = entry.expected.to_lowercase();

        let claimed_degree: Option<u32> = if lower.contains("linear") {
            Some(1)
        } else if lower.contains("quadratic") {
            Some(2)
        } else {
            None
        };

        match claimed_degree {
            Some(expected_d) => {
                let actual = deg(&mut pool, in_id, x);
                assert_eq!(
                    actual,
                    Some(expected_d),
                    "{:?} claims to be {:?} (degree {}) in x, but deg() returned {:?}",
                    entry.input, entry.expected, expected_d, actual
                );
                t.verified += 1;
                println!(
                    "FORM-OK: {} (degree {} in x)",
                    entry.input, expected_d
                );
            }
            None => {
                t.smoke += 1;
                println!("SMOKE: {} (expected {:?} not a form claim)", entry.input, entry.expected);
            }
        }
    }
    t.print_summary(path);
    t.assert_min_verified(path, min_verified);
}

// Per-manifest VERIFIED floors. Set just below current baselines so a
// modest amount of churn doesn't trip the assertion, but a wholesale
// downgrade of entries to `ignore = true` / `SMOKE:` does. Bump when a
// manifest's verified count climbs durably.
const MIN_VERIFIED_ALG_EXPR: usize = 10;
const MIN_VERIFIED_SIMPLIFY: usize = 10;
const MIN_VERIFIED_DIFF: usize = 5;
const MIN_VERIFIED_POLY_DIV: usize = 10;
const MIN_VERIFIED_SOLVE: usize = 5;

#[test]
fn golden_alg_expr() {
    run_simplify_match(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/alg_expr.toml"),
        MIN_VERIFIED_ALG_EXPR,
    );
}

#[test]
fn golden_simplify() {
    run_simplify_match(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/simplify.toml"),
        MIN_VERIFIED_SIMPLIFY,
    );
}

#[test]
fn golden_diff() {
    run_diff_match(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/diff.toml"),
        MIN_VERIFIED_DIFF,
    );
}

#[test]
fn golden_poly_div() {
    run_poly_check(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/poly_div.toml"),
        MIN_VERIFIED_POLY_DIV,
    );
}

#[test]
fn golden_solve() {
    run_solve_form(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/solve_linear_quadratic.toml"),
        MIN_VERIFIED_SOLVE,
    );
}
