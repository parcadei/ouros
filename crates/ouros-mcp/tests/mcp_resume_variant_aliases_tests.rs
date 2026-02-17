use ouros_mcp::handler::McpHandler;
use serde_json::json;

#[test]
fn resume_accepts_python_style_int_alias() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["fetch"]}))
        .unwrap();

    let progress = handler.call_tool("execute", json!({"code": "fetch(1) + 1"})).unwrap();
    let call_id = progress["call_id"].as_u64().expect("call_id should be present");

    let result = handler
        .call_tool(
            "resume",
            json!({
                "call_id": call_id,
                "result": {"int": 41}
            }),
        )
        .unwrap();

    assert_eq!(result, json!({"status": "complete", "result": 42, "repr": "42"}));
}

#[test]
fn resume_accepts_python_style_str_alias() {
    let mut handler = McpHandler::new("<mcp>");
    handler
        .call_tool("reset", json!({"external_functions": ["fetch"]}))
        .unwrap();

    let progress = handler.call_tool("execute", json!({"code": "fetch(1) + '!'"})).unwrap();
    let call_id = progress["call_id"].as_u64().expect("call_id should be present");

    let result = handler
        .call_tool(
            "resume",
            json!({
                "call_id": call_id,
                "result": {"str": "ok"}
            }),
        )
        .unwrap();

    assert_eq!(result, json!({"status": "complete", "result": "ok!", "repr": "'ok!'"}));
}
