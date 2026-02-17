//! Fuzz target: parse and execute arbitrary Python source.
//!
//! This target exercises the full pipeline — parsing, compilation, and execution —
//! with strict resource limits to prevent the fuzzer from triggering legitimate
//! resource exhaustion (which is handled gracefully, not a bug).
//!
//! Findings from this target indicate real safety issues: panics, stack overflows,
//! infinite loops that evade resource limits, or memory corruption.

#![no_main]

use libfuzzer_sys::fuzz_target;

use ouros::{LimitedTracker, Runner, NoPrint, ResourceLimits};

fuzz_target!(|data: &[u8]| {
    // Only fuzz valid UTF-8 — Python source is always text.
    let Ok(code) = std::str::from_utf8(data) else {
        return;
    };

    // Skip excessively large inputs — they slow the fuzzer without finding
    // interesting bugs. Most parser/runtime bugs reproduce with small inputs.
    if code.len() > 4096 {
        return;
    }

    // Compile the source. Parse/compile failures are expected and not bugs.
    let Ok(runner) = Runner::new(code.to_owned(), "fuzz.py", vec![], vec![]) else {
        return;
    };

    // Execute with tight resource limits so the fuzzer doesn't waste time on
    // legitimate resource exhaustion paths.
    let limits = ResourceLimits::new()
        .max_allocations(10_000)
        .max_memory(10 * 1024 * 1024) // 10 MiB
        .max_recursion_depth(Some(50));
    let tracker = LimitedTracker::new(limits);

    // Discard the result — we only care that execution doesn't panic or crash.
    let _ = runner.run(vec![], tracker, &mut NoPrint);
});
