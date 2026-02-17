// Use codspeed-criterion-compat when running on CodSpeed (CI), real criterion otherwise (for flamegraphs)
#[cfg(not(codspeed))]
use std::ffi::CString;

#[cfg(codspeed)]
use codspeed_criterion_compat::{Bencher, Criterion, black_box, criterion_group, criterion_main};
#[cfg(not(codspeed))]
use criterion::{Bencher, Criterion, black_box, criterion_group, criterion_main};
use ouros::Runner;
#[cfg(not(codspeed))]
use pyo3::prelude::*;

/// Runs a benchmark using the Ouros interpreter.
///
/// This variant intentionally uses non-foldable benchmark bodies so the measured
/// time reflects runtime arithmetic execution rather than constant-return
/// short-circuiting.
fn run_ouros(bench: &mut Bencher, code: &str, expected: i64) {
    let ex = Runner::new(code.to_owned(), "test.py", vec![], vec![]).unwrap();
    let r = ex.run_no_limits(vec![]).unwrap();
    let int_value: i64 = r.as_ref().try_into().unwrap();
    assert_eq!(int_value, expected);

    bench.iter(|| {
        let r = ex.run_no_limits(vec![]).unwrap();
        let int_value: i64 = r.as_ref().try_into().unwrap();
        black_box(int_value);
    });
}

/// Runs a benchmark using CPython.
///
/// Code is wrapped into a `main()` function and the last expression is converted
/// into an explicit return for parity with the Ouros benchmark harness.
#[cfg(not(codspeed))]
fn run_cpython(bench: &mut Bencher, code: &str, expected: i64) {
    Python::attach(|py| {
        let wrapped = wrap_for_cpython(code);
        let code_cstr = CString::new(wrapped).expect("Invalid C string in code");
        let fun: Py<PyAny> = PyModule::from_code(py, &code_cstr, c"test.py", c"main")
            .unwrap()
            .getattr("main")
            .unwrap()
            .into();

        let r_py = fun.call0(py).unwrap();
        let r: i64 = r_py.extract(py).unwrap();
        assert_eq!(r, expected);

        bench.iter(|| {
            let r_py = fun.call0(py).unwrap();
            let r: i64 = r_py.extract(py).unwrap();
            black_box(r);
        });
    });
}

/// Wraps code in `def main():` and converts the last non-comment expression to a return.
#[cfg(not(codspeed))]
fn wrap_for_cpython(code: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut last_expr = String::new();

    for line in code.lines() {
        if line.starts_with("# Return=") || line.starts_with("# Raise=") || line.starts_with("# skip=") {
            continue;
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            last_expr = line.to_string();
        }
        lines.push(format!("    {line}"));
    }

    if let Some(last) = lines.iter().rposition(|l| l.trim() == last_expr.trim()) {
        lines[last] = format!("    return {}", last_expr.trim());
    }

    format!("def main():\n{}", lines.join("\n"))
}

/// Two locals added at runtime.
///
/// This shape avoids Ouros's `constant_return` short-circuit by requiring
/// statement execution before producing the final expression.
const ADD_TWO_LOCALS: &str = "
x = 1
y = 2
x + y
";

/// Tight arithmetic loop over local ints.
///
/// This exercises repeated integer additions in a non-foldable form.
const ADD_TWO_LOOP_1000: &str = "
x = 1
y = 2
total = 0
for _ in range(1000):
    total += x + y
total
";

/// Configures the non-foldable arithmetic benchmark group.
fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("add_two_locals__ouros", |b| run_ouros(b, ADD_TWO_LOCALS, 3));
    #[cfg(not(codspeed))]
    c.bench_function("add_two_locals__cpython", |b| run_cpython(b, ADD_TWO_LOCALS, 3));

    c.bench_function("add_two_loop_1000__ouros", |b| run_ouros(b, ADD_TWO_LOOP_1000, 3000));
    #[cfg(not(codspeed))]
    c.bench_function("add_two_loop_1000__cpython", |b| {
        run_cpython(b, ADD_TWO_LOOP_1000, 3000);
    });
}

// Use pprof flamegraph profiler when running locally (not on CodSpeed)
#[cfg(not(codspeed))]
criterion_group!(benches, criterion_benchmark);

// Use default config when running on CodSpeed
#[cfg(codspeed)]
criterion_group!(benches, criterion_benchmark);

criterion_main!(benches);
