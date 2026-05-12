#![no_main]
use libfuzzer_sys::fuzz_target;
use monomix_kernel::{expr::ExprPool, parser::parse};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let mut pool = ExprPool::new();
        let _result = parse(s, &mut pool);
        // Must not panic; ParseResult is the contract.
    }
});
