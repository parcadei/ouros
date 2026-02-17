use std::{
    io::{self, Write},
    process::ExitCode,
};

use ouros::{Object, ReplProgress, ReplSession, StdPrint};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        // File execution mode
        let path = &args[1];
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {path}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let mut session = ReplSession::new(vec![], path);
        if let Err(err) = execute_snippet(&mut session, &source) {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }

    // Interactive mode
    let mut session = ReplSession::new(vec![], "<stdin>");
    let mut source = String::new();

    loop {
        let prompt = if source.is_empty() { ">>> " } else { "... " };
        let Some(line) = read_line(prompt) else {
            println!();
            break;
        };

        if source.is_empty() && line.trim().is_empty() {
            continue;
        }

        if !source.is_empty() {
            source.push('\n');
        }
        source.push_str(&line);

        if needs_more_input(&source) {
            continue;
        }

        if let Err(err) = execute_snippet(&mut session, &source) {
            eprintln!("{err}");
        }
        source.clear();
    }

    ExitCode::SUCCESS
}

/// Executes one source snippet and handles interactive yield/resume with prompts.
fn execute_snippet(session: &mut ReplSession, source: &str) -> Result<(), ouros::ReplError> {
    let mut progress = session.execute_interactive(source, &mut StdPrint)?;

    loop {
        match progress {
            ReplProgress::Complete(value) => {
                if value != Object::None {
                    println!("{value}");
                }
                return Ok(());
            }
            ReplProgress::FunctionCall {
                function_name,
                args,
                kwargs,
                call_id,
            } => {
                println!("external call #{call_id}: {function_name} args={args:?} kwargs={kwargs:?}");
                let return_value = prompt_return_value()?;
                progress = session.resume(return_value, &mut StdPrint)?;
            }
            ReplProgress::ProxyCall {
                proxy_id,
                method,
                args,
                kwargs,
                call_id,
            } => {
                println!("proxy call #{call_id}: <proxy #{proxy_id}>.{method} args={args:?} kwargs={kwargs:?}");
                let return_value = prompt_return_value()?;
                progress = session.resume(return_value, &mut StdPrint)?;
            }
            ReplProgress::ResolveFutures {
                pending_call_ids,
                pending_futures,
            } => {
                println!("pending futures: {pending_call_ids:?}");
                for info in &pending_futures {
                    println!("  call_id={}: {}({:?})", info.call_id, info.function_name, info.args);
                }
                // For the interactive binary, resolve all futures with None
                let results: Vec<(u32, ouros::ExternalResult)> = pending_call_ids
                    .into_iter()
                    .map(|id| (id, ouros::ExternalResult::Return(Object::None)))
                    .collect();
                progress = session.resume_futures(results, &mut StdPrint)?;
            }
        }
    }
}

/// Reads and parses a host return value for interactive resume.
fn prompt_return_value() -> Result<Object, ouros::ReplError> {
    loop {
        let Some(line) = read_line("return> ") else {
            return Ok(Object::None);
        };
        match parse_return_value(&line) {
            Ok(value) => return Ok(value),
            Err(err) => eprintln!("{err}"),
        }
    }
}

/// Parses a user-entered return value.
///
/// Supported forms:
/// - `None`, `True`, `False`
/// - `proxy:<id>` for proxy handles
/// - integer and float literals
/// - quoted strings (`'text'` / `"text"`)
/// - full `Object` JSON (e.g. `{"Int":1}`)
/// - fallback: bare text becomes `Object::String`
fn parse_return_value(raw: &str) -> Result<Object, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err("empty return value".to_owned());
    }
    if value == "None" {
        return Ok(Object::None);
    }
    if value == "True" {
        return Ok(Object::Bool(true));
    }
    if value == "False" {
        return Ok(Object::Bool(false));
    }
    if let Some(proxy_raw) = value.strip_prefix("proxy:") {
        let proxy_id = proxy_raw
            .trim()
            .parse::<u32>()
            .map_err(|err| format!("invalid proxy id: {err}"))?;
        return Ok(Object::Proxy(proxy_id));
    }
    if (value.starts_with('"') && value.ends_with('"')) || (value.starts_with('\'') && value.ends_with('\'')) {
        return Ok(Object::String(value[1..value.len() - 1].to_owned()));
    }
    if let Ok(int_value) = value.parse::<i64>() {
        return Ok(Object::Int(int_value));
    }
    if let Ok(float_value) = value.parse::<f64>() {
        return Ok(Object::Float(float_value));
    }
    if let Ok(json_value) = serde_json::from_str::<Object>(value) {
        return Ok(json_value);
    }
    Ok(Object::String(value.to_owned()))
}

/// Heuristic multiline detector for interactive input.
fn needs_more_input(source: &str) -> bool {
    let trimmed = source.trim_end();
    if trimmed.ends_with('\\') {
        return true;
    }

    let mut balance = 0i32;
    for ch in trimmed.chars() {
        match ch {
            '(' | '[' | '{' => balance += 1,
            ')' | ']' | '}' => balance -= 1,
            _ => {}
        }
    }
    if balance > 0 {
        return true;
    }

    trimmed
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.trim_end().ends_with(':'))
}

/// Reads one line from stdin after printing a prompt.
///
/// Returns `None` on EOF (Ctrl+D).
fn read_line(prompt: &str) -> Option<String> {
    print!("{prompt}");
    if io::stdout().flush().is_err() {
        return None;
    }
    let mut input = String::new();
    let read = io::stdin().read_line(&mut input).ok()?;
    if read == 0 {
        return None;
    }
    Some(input.trim_end_matches(['\r', '\n']).to_owned())
}
