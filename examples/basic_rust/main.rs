use ouros::{NoLimitTracker, Object, Runner, StdPrint};

fn main() {
    // --- Basic execution ---
    let code = "x + y";
    let runner = Runner::new(
        code.to_owned(),
        "basic.py",
        vec!["x".to_owned(), "y".to_owned()],
        vec![],
    )
    .unwrap();

    let result = runner
        .run(
            vec![Object::Int(10), Object::Int(20)],
            NoLimitTracker,
            &mut StdPrint,
        )
        .unwrap();

    assert_eq!(result, Object::Int(30));
    println!("Basic: {result:?}"); // Int(30)

    // --- Fibonacci ---
    let fib_code = r#"
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

fib(x)
"#;

    let fib_runner = Runner::new(
        fib_code.to_owned(),
        "fib.py",
        vec!["x".to_owned()],
        vec![],
    )
    .unwrap();

    let fib_result = fib_runner
        .run(vec![Object::Int(10)], NoLimitTracker, &mut StdPrint)
        .unwrap();

    assert_eq!(fib_result, Object::Int(55));
    println!("Fibonacci(10): {fib_result:?}"); // Int(55)

    // --- External functions ---
    let ext_code = r#"
result = fetch(url)
len(result)
"#;

    let ext_runner = Runner::new(
        ext_code.to_owned(),
        "external.py",
        vec!["url".to_owned()],
        vec!["fetch".to_owned()],
    )
    .unwrap();

    // start() returns a snapshot when fetch() is called
    let snapshot = ext_runner
        .start(
            vec![Object::Str("https://example.com".to_owned())],
            NoLimitTracker,
            &mut StdPrint,
        )
        .unwrap();

    match snapshot {
        ouros::RunResult::Snapshot(snap) => {
            println!("Paused at: {}", snap.function_name());
            println!("Args: {:?}", snap.args());

            // Resume with a return value
            let final_result = snap
                .resume(Object::Str("hello world".to_owned()), NoLimitTracker, &mut StdPrint)
                .unwrap();

            println!("Result: {final_result:?}"); // Int(11)
        }
        ouros::RunResult::Complete(val) => {
            println!("Completed immediately: {val:?}");
        }
    }

    println!("All examples passed.");
}
