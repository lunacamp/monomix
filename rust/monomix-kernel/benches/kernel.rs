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
