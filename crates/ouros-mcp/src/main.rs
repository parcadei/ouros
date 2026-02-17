use std::{
    io::{self, BufRead, BufReader, Write},
    path::PathBuf,
};

use ouros_mcp::handler::McpHandler;
use serde::Deserialize;
use serde_json::{Value, json};

/// JSON-RPC request payload used by this minimal MCP server.
#[derive(Debug, Deserialize)]
struct RpcRequest {
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    let mut handler = McpHandler::new("<mcp>");

    let storage_dir = parse_storage_dir();
    if let Some(dir) = storage_dir {
        handler.set_storage_dir(dir).map_err(io::Error::other)?;
    }

    while let Some(body) = read_framed_message(&mut reader)? {
        let raw_message = match serde_json::from_slice::<Value>(&body) {
            Ok(message) => message,
            Err(err) => {
                let response = error_response(&Value::Null, -32700, &format!("parse error: {err}"));
                write_framed_message(&mut writer, &response)?;
                continue;
            }
        };

        if is_json_rpc_notification(&raw_message) {
            continue;
        }

        let response = match serde_json::from_value::<RpcRequest>(raw_message) {
            Ok(request) => handle_request(&mut handler, request),
            Err(err) => error_response(&Value::Null, -32700, &format!("parse error: {err}")),
        };
        write_framed_message(&mut writer, &response)?;
    }

    Ok(())
}

/// Returns true when the payload is a JSON-RPC 2.0 notification.
///
/// Notifications contain a string `method` and intentionally omit `id`.
/// The server must not produce any response for these messages.
fn is_json_rpc_notification(payload: &Value) -> bool {
    let Some(object) = payload.as_object() else {
        return false;
    };

    object.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
        && object.get("method").is_some_and(Value::is_string)
        && !object.contains_key("id")
}

fn handle_request(handler: &mut McpHandler, request: RpcRequest) -> Value {
    match request.method.as_str() {
        "initialize" => success_response(
            &request.id,
            &json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "ouros-mcp",
                    "version": "0.0.4"
                }
            }),
        ),
        "notifications/initialized" => success_response(&request.id, &json!({})),
        "tools/list" => success_response(&request.id, &json!({ "tools": handler.list_tools() })),
        "tools/call" => {
            #[derive(Deserialize)]
            struct CallParams {
                name: String,
                #[serde(default)]
                arguments: Value,
            }

            let params: Result<CallParams, _> = serde_json::from_value(request.params);
            match params {
                Ok(params) => match handler.call_tool(&params.name, params.arguments) {
                    Ok(result) => success_response(&request.id, &json!({ "content": result })),
                    Err(err) => error_response(&request.id, -32000, &err),
                },
                Err(err) => error_response(&request.id, -32602, &format!("invalid params: {err}")),
            }
        }
        _ => error_response(&request.id, -32601, "method not found"),
    }
}

fn success_response(id: &Value, result: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn error_response(id: &Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        },
    })
}

/// Reads one Content-Length framed message body from stdin.
fn read_framed_message(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut content_length = None;
    loop {
        let mut header_line = String::new();
        let read = reader.read_line(&mut header_line)?;
        if read == 0 {
            return Ok(None);
        }
        let trimmed = header_line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            let length = value
                .trim()
                .parse::<usize>()
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, format!("invalid Content-Length: {err}")))?;
            content_length = Some(length);
        }
    }

    let Some(content_length) = content_length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Content-Length header",
        ));
    };

    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Writes one Content-Length framed JSON message to stdout.
fn write_framed_message(writer: &mut impl Write, payload: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(payload)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, format!("serialize error: {err}")))?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

/// Parses the `--storage-dir` flag from command-line arguments.
///
/// Returns `Some(path)` if `--storage-dir <path>` was provided, otherwise
/// falls back to `$OUROS_STORAGE_DIR` env var, then to `$HOME/.ouros/sessions/`.
/// Returns `None` only if no home directory can be determined.
fn parse_storage_dir() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len().saturating_sub(1) {
        if args[i] == "--storage-dir" {
            return Some(PathBuf::from(&args[i + 1]));
        }
    }

    if let Ok(dir) = std::env::var("OUROS_STORAGE_DIR") {
        return Some(PathBuf::from(dir));
    }

    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".ouros").join("sessions"))
}
