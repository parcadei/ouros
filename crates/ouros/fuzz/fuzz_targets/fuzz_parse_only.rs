//! Fuzz target: parse and compile arbitrary Python source (no execution).
//!
//! This target exercises only the parser and bytecode compiler, skipping execution.
//! It runs much faster than `fuzz_parse_run` and is effective for finding panics
//! in the Ruff parser integration, AST walking, and bytecode generation.
//!
//! A crash here indicates a bug in parse/compile — these should never panic
//! regardless of input, only return errors.

#![no_main]

use libfuzzer_sys::fuzz_target;

use ouros::Runner;

fuzz_target!(|data: &[u8]| {
    let Ok(code) = std::str::from_utf8(data) else {
        return;
    };

    // Skip excessively large inputs.
    if code.len() > 8192 {
        return;
    }

    // Attempt to parse and compile. We don't care about the result —
    // only that it doesn't panic.
    let _ = Runner::new(code.to_owned(), "fuzz.py", vec![], vec![]);
});
