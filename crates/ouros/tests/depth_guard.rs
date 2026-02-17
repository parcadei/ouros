/// Tests for the data recursion depth guard.
///
/// The depth guard prevents stack overflow when repr, eq, cmp, or hash are called
/// on deeply nested (but non-circular) data structures. Without the guard, a list
/// nested 1000+ levels deep would overflow the Rust call stack during `repr()`.
use ouros::{CollectStringPrint, NoLimitTracker, Runner};

/// Helper to run Python code and return the printed output.
fn run_code(code: &str) -> String {
    let ex = Runner::new(code.to_owned(), "test.py", vec![], vec![]).unwrap();
    let mut print = CollectStringPrint::new();
    ex.run(vec![], NoLimitTracker, &mut print).expect("should succeed");
    print.output().trim().to_string()
}

/// Deeply nested list repr should not crash (stack overflow).
///
/// Creates a list nested 200 levels deep and calls repr() on it. Without the depth
/// guard this would overflow the stack. With the guard, the output is truncated with
/// `...` at the depth limit.
#[test]
fn deeply_nested_list_repr_does_not_crash() {
    let code = r"
a = [42]
for _ in range(200):
    a = [a]
r = repr(a)
# Should contain opening brackets and eventually truncate
assert r.startswith('['), f'repr should start with [, got: {r[:20]}'
print('ok')
";
    let output = run_code(code);
    assert_eq!(output, "ok");
}

/// Deeply nested dict repr should not crash.
#[test]
fn deeply_nested_dict_repr_does_not_crash() {
    let code = "
d = {'leaf': 1}
for _ in range(200):
    d = {'nested': d}
r = repr(d)
assert r.startswith('{'), f'repr should start with curly brace, got: {r[:20]}'
print('ok')
";
    let output = run_code(code);
    assert_eq!(output, "ok");
}

/// Deeply nested tuple repr should not crash.
#[test]
fn deeply_nested_tuple_repr_does_not_crash() {
    let code = r"
t = (42,)
for _ in range(200):
    t = (t,)
r = repr(t)
assert r.startswith('('), f'repr should start with paren, got: {r[:20]}'
print('ok')
";
    let output = run_code(code);
    assert_eq!(output, "ok");
}

/// Deeply nested list equality should not crash.
///
/// Two identical deeply nested lists should be comparable without stack overflow.
/// The depth guard returns `false` when the limit is exceeded.
#[test]
fn deeply_nested_list_eq_does_not_crash() {
    let code = r"
a = [42]
b = [42]
for _ in range(200):
    a = [a]
    b = [b]
# Comparison should not crash; result may be False due to depth limit
result = (a == b)
print('ok')
";
    let output = run_code(code);
    assert_eq!(output, "ok");
}

/// Deeply nested list comparison (cmp) should not crash.
#[test]
fn deeply_nested_list_cmp_does_not_crash() {
    let code = r"
a = [1]
b = [2]
for _ in range(200):
    a = [a]
    b = [b]
try:
    result = (a < b)
except TypeError:
    pass
print('ok')
";
    let output = run_code(code);
    assert_eq!(output, "ok");
}

/// Normal shallow structures should be unaffected by the depth guard.
#[test]
fn shallow_structures_unaffected() {
    let code = "
# Normal operations should work exactly as before
a = [1, 2, 3]
assert repr(a) == '[1, 2, 3]'

b = {'x': [1, 2], 'y': (3, 4)}
assert repr(b) == \"{'x': [1, 2], 'y': (3, 4)}\"

c = [[1, 2], [3, 4]]
d = [[1, 2], [3, 4]]
assert c == d

print('ok')
";
    let output = run_code(code);
    assert_eq!(output, "ok");
}
