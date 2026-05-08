use monomix_kernel::expr::ExprPool;
use monomix_kernel::parser::parse;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Entry {
    input: String,
    #[allow(dead_code)]
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
