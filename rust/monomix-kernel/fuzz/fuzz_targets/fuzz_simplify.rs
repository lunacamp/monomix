#![no_main]
use libfuzzer_sys::fuzz_target;
use monomix_kernel::{
    expr::ExprPool, parser::parse,
    simplify::{simplify, SimplifierConfig, SimplifyCache},
};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut pool = ExprPool::new();
        let result = parse(s, &mut pool);
        let config = SimplifierConfig::default();
        let mut cache = SimplifyCache::new();
        for stmt in &result.statements {
            let _ = simplify(&mut pool, stmt.expr, &config, &mut cache);
        }
    }
});
