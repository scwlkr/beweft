mod client;
mod mcp;
mod tools;
mod upload;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::io::Read;
const MAX_JSON_INPUT: u64 = 2 * 1024 * 1024;

fn read_json(path: &str) -> Result<Value> {
    let mut bytes = Vec::new();
    if path == "-" {
        std::io::stdin()
            .take(MAX_JSON_INPUT + 1)
            .read_to_end(&mut bytes)?;
    } else {
        std::fs::File::open(path)
            .context("Cannot open JSON file")?
            .take(MAX_JSON_INPUT + 1)
            .read_to_end(&mut bytes)?;
    }
    ensure!(
        bytes.len() as u64 <= MAX_JSON_INPUT,
        "JSON input exceeds 2 MiB"
    );
    serde_json::from_slice(&bytes).context("Invalid JSON input")
}
async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || matches!(args[0].as_str(), "help" | "--help" | "-h") {
        println!(
            "beweft <tool> [--json FILE|-]\nbeweft schema <tool>\nbeweft api <GET|POST|PATCH|PUT|DELETE> /path [--json FILE|-]\nbeweft mcp\n\nTools: {}\nUse underscores or hyphens. Outputs JSON. Tokens: BEWEFT_TOKEN_FILE (chmod 600) or BEWEFT_TOKEN.\nServer origin: BEWEFT_URL (default http://127.0.0.1:3000). No automatic write retries.",
            tools::NAMES.join(", ")
        );
        return Ok(());
    }
    let command = args[0].replace('-', "_");
    if command == "schema" {
        ensure!(args.len() == 2, "Usage: beweft schema <tool>");
        let name = args[1].replace('-', "_");
        ensure!(tools::NAMES.contains(&name.as_str()), "Unknown tool");
        println!(
            "{}",
            serde_json::to_string_pretty(&tools::description(&name))?
        );
        return Ok(());
    }
    let api = client::Api::from_env()?;
    if command == "mcp" {
        ensure!(args.len() == 1, "mcp accepts no arguments");
        return mcp::serve(api).await;
    }
    let result = if command == "api" {
        ensure!(
            args.len() == 3 || (args.len() == 5 && args[3] == "--json"),
            "Usage: beweft api METHOD /path [--json FILE|-]"
        );
        ensure!(
            matches!(
                args[1].as_str(),
                "GET" | "POST" | "PATCH" | "PUT" | "DELETE"
            ),
            "Unsupported method"
        );
        api.request(
            args[1].parse()?,
            &args[2],
            if args.len() == 5 {
                Some(read_json(&args[4])?)
            } else {
                None
            },
        )
        .await?
    } else {
        ensure!(
            args.len() == 1 || (args.len() == 3 && args[1] == "--json"),
            "Usage: beweft <tool> [--json FILE|-]; use schema <tool> for input"
        );
        tools::invoke(
            &api,
            &command,
            if args.len() == 3 {
                read_json(&args[2])?
            } else {
                json!({})
            },
            false,
            None,
        )
        .await?
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("beweft: {error}");
        std::process::exit(1);
    }
}
