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

use monomix_kernel::parser::parse;

fn bench_parse_100_term_poly(c: &mut Criterion) {
    // Build a 100-term polynomial source string: 1*x^100 + 2*x^99 + ...
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

fn bench_parse_20_assignments(c: &mut Criterion) {
    let src = (0..20)
        .map(|i| format!("x{} := {}*y + {};", i, i, i + 1))
        .collect::<Vec<_>>()
        .join(" ");
    c.bench_function("parse 20 assignments", |b| {
        b.iter(|| {
            let mut pool = monomix_kernel::expr::ExprPool::new();
            black_box(parse(&src, &mut pool));
        });
    });
}

criterion_group!(
    benches,
    bench_intern_integers,
    bench_intern_add_nodes,
    bench_map_bottom_up_identity,
    bench_parse_100_term_poly,
    bench_parse_20_assignments,
);
criterion_main!(benches);
