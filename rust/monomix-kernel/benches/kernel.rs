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

use monomix_kernel::simplify::{simplify, SimplifierConfig, SimplifyCache};

fn bench_simplify_50_term_sum(c: &mut Criterion) {
    c.bench_function("simplify 50-term sum", |b| {
        b.iter(|| {
            let mut pool = monomix_kernel::expr::ExprPool::new();
            let x = pool.symbol("x");
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

criterion_group!(
    benches,
    bench_intern_integers,
    bench_intern_add_nodes,
    bench_map_bottom_up_identity,
    bench_parse_100_term_poly,
    bench_parse_20_assignments,
    bench_expand_x_plus_1_pow_20,
    bench_diff_20_term_poly,
    bench_simplify_50_term_sum,
    bench_solve_quadratic,
);
criterion_main!(benches);
