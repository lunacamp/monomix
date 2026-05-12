#![no_main]
use libfuzzer_sys::fuzz_target;
use monomix_kernel::{expr::ExprPool, parser::parse, diff::differentiate};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut pool = ExprPool::new();
        let result = parse(s, &mut pool);
        let x = pool.symbol("x");
        for stmt in &result.statements {
            let _ = differentiate(&mut pool, stmt.expr, x);
        }
    }
});
