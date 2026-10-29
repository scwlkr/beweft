use crate::{client::Api, tools};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{BufRead, Read, Write},
    time::{Duration, Instant},
};
const MAX_MESSAGE: u64 = 2 * 1024 * 1024;
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
pub async fn serve(api: Api) -> Result<()> {
    let upload_root = crate::upload::UploadRoot::from_env()?;
    let calls_per_minute = std::env::var("BEWEFT_MCP_CALLS_PER_MINUTE")
        .unwrap_or_else(|_| "120".into())
        .parse::<usize>()
        .context("BEWEFT_MCP_CALLS_PER_MINUTE must be between 1 and 10000")?;
    ensure!(
        (1..=10000).contains(&calls_per_minute),
        "BEWEFT_MCP_CALLS_PER_MINUTE must be between 1 and 10000"
    );
    let mut calls = VecDeque::new();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let mut state = 0;
    loop {
        let mut line = Vec::new();
        let read = input
            .by_ref()
            .take(MAX_MESSAGE + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        if read as u64 > MAX_MESSAGE {
            // Stop on oversized frames, without allocating their unbounded remainder.
            writeln!(
                output,
                "{}",
                error(Value::Null, -32600, "Message exceeds 2 MiB")
            )?;
            output.flush()?;
            break;
        }
        let message: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) => {
                writeln!(output, "{}", error(Value::Null, -32700, "Invalid JSON"))?;
                output.flush()?;
                continue;
            }
        };
        let id = message.get("id").cloned();
        let method = message["method"].as_str();
        if !message.is_object()
            || message["jsonrpc"] != "2.0"
            || method.is_none()
            || id
                .as_ref()
                .is_some_and(|v| !(v.is_string() || v.is_i64() || v.is_u64()))
        {
            writeln!(
                output,
                "{}",
                error(Value::Null, -32600, "Invalid JSON-RPC request")
            )?;
            output.flush()?;
            continue;
        }
        let method = method.unwrap();
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        if id.is_none() {
            if method == "notifications/initialized" && state == 1 {
                state = 2;
            }
            // Notifications never execute writes and never receive a response.
            continue;
        }
        let id = id.unwrap();
        let result = match method {
            "ping" => Ok(json!({})),
            "initialize" if state == 0 => {
                if params["protocolVersion"].is_string()
                    && params["capabilities"].is_object()
                    && params["clientInfo"]["name"].is_string()
                    && params["clientInfo"]["version"].is_string()
                {
                    state = 1;
                    Ok(
                        json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"beweft","version":env!("CARGO_PKG_VERSION")},"instructions":"Draft actions never schedule. Scheduling requires explicit intent and version. Writes are never retried; inspect authoritative state after unknown outcomes."}),
                    )
                } else {
                    Err((-32602, "Invalid initialize parameters"))
                }
            }
            "initialize" => Err((-32600, "Already initialized")),
            _ if state != 2 => Err((
                -32002,
                "Complete initialize and notifications/initialized first",
            )),
            "tools/list" => {
                if !params.is_object() || params.get("cursor").is_some() {
                    Err((-32602, "This tool list has no pagination cursor"))
                } else {
                    Ok(
                        json!({"tools":tools::NAMES.iter().map(|n|tools::description(n)).collect::<Vec<_>>()}),
                    )
                }
            }
            "tools/call" => {
                if !params.is_object()
                    || params
                        .get("arguments")
                        .is_some_and(|args| !args.is_object())
                {
                    Err((-32602, "Invalid tool call parameters"))
                } else if let Some(name) = params["name"]
                    .as_str()
                    .filter(|name| tools::NAMES.contains(name))
                {
                    let now = Instant::now();
                    while calls
                        .front()
                        .is_some_and(|time| now.duration_since(*time) >= Duration::from_secs(60))
                    {
                        calls.pop_front();
                    }
                    if calls.len() >= calls_per_minute {
                        Ok(
                            json!({"content":[{"type":"text","text":"MCP tool rate limit reached; wait before trying again."}],"isError":true}),
                        )
                    } else {
                        calls.push_back(now);
                        match tools::invoke(
                            &api,
                            name,
                            params
                                .get("arguments")
                                .cloned()
                                .unwrap_or_else(|| json!({})),
                            true,
                            upload_root.as_ref(),
                        )
                        .await
                        {
                            Ok(value) => {
                                let mut result = json!({"content":[{"type":"text","text":serde_json::to_string(&value)?}],"isError":false});
                                if value.is_object() {
                                    result["structuredContent"] = value;
                                }
                                Ok(result)
                            }
                            Err(e) => Ok(
                                json!({"content":[{"type":"text","text":e.to_string()}],"isError":true}),
                            ),
                        }
                    }
                } else {
                    Err((-32602, "Unknown tool name"))
                }
            }
            _ => Err((-32601, "Method not found")),
        };
        let response = match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err((code, message)) => error(id, code, message),
        };
        writeln!(output, "{response}")?;
        output.flush()?;
    }
    Ok(())
}
