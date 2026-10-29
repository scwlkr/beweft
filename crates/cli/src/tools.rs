use crate::client::Api;
use anyhow::{Context, Result, bail, ensure};
use reqwest::Method;
use serde_json::{Value, json};
use std::path::Path;

pub const NAMES: &[&str] = &[
    "brands",
    "channels",
    "drafts",
    "post",
    "queue",
    "history",
    "attempts",
    "health",
    "draft_create",
    "draft_update",
    "schedule",
    "media_upload",
];
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn request_schema(name: &str) -> Value {
    fn expand(value: &Value, schemas: &Value, depth: u8) -> Value {
        if depth > 15 {
            return json!({});
        }
        if let Some(reference) = value["$ref"].as_str() {
            return expand(
                &schemas[reference.rsplit('/').next().unwrap()],
                schemas,
                depth + 1,
            );
        }
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), expand(v, schemas, depth + 1)))
                    .collect(),
            ),
            Value::Array(array) => Value::Array(
                array
                    .iter()
                    .map(|v| expand(v, schemas, depth + 1))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    let spec: Value = serde_json::from_str(include_str!("../../../docs/openapi.json"))
        .expect("generated API contract");
    expand(
        &spec["components"]["schemas"][name],
        &spec["components"]["schemas"],
        0,
    )
}
pub fn description(name: &str) -> Value {
    let id = json!({"type":"string","format":"uuid"});
    let filter = object(json!({"brand_id":id}), &[]);
    let (summary, schema, read) = match name {
        "brands" => (
            "List brands accessible to this token.",
            object(json!({}), &[]),
            true,
        ),
        "channels" => ("List channels and connection health.", filter, true),
        "drafts" => (
            "List posts whose authoritative status is draft.",
            filter,
            true,
        ),
        "post" => (
            "Read one post including its revision and channel variants.",
            object(json!({"id":id}), &["id"]),
            true,
        ),
        "queue" => (
            "Read queued and sending publications, including immutable content and timing.",
            filter,
            true,
        ),
        "history" => (
            "Read completed, failed, manual, uncertain and canceled publication outcomes.",
            filter,
            true,
        ),
        "attempts" => (
            "Read durable attempt history for a publication; use after any unknown write outcome.",
            object(json!({"id":id}), &["id"]),
            true,
        ),
        "health" => (
            "Read independent worker heartbeat and job counts.",
            object(json!({}), &[]),
            true,
        ),
        "draft_create" => (
            "Create a draft only. Never schedules or publishes.",
            object(
                json!({"body":request_schema("CreatePostRequest")}),
                &["body"],
            ),
            false,
        ),
        "draft_update" => (
            "Update post content at an explicitly supplied version. Existing scheduled snapshots remain unchanged.",
            object(
                json!({"id":id,"body":request_schema("UpdatePostRequest")}),
                &["id", "body"],
            ),
            false,
        ),
        "schedule" => (
            "Schedule the approved post version. Explicit intent=queue reserves posting-rule slots; intent=exact requires scheduled_at. Can publish when due. Never retry an unknown outcome without reading the queue.",
            object(
                json!({"id":id,"intent":{"type":"string","enum":["queue","exact"]},"body":request_schema("SchedulePostRequest")}),
                &["id", "intent", "body"],
            ),
            false,
        ),
        "media_upload" => (
            "Upload a local file for a brand. MCP requires BEWEFT_UPLOAD_ROOT and accepts files only within that directory. Never schedules.",
            object(
                json!({"brand_id":id,"path":{"type":"string","minLength":1}}),
                &["brand_id", "path"],
            ),
            false,
        ),
        _ => unreachable!(),
    };
    json!({"name":name,"description":summary,"inputSchema":schema,"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":read,"openWorldHint":true}})
}
fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .with_context(|| format!("{key} must be a string"))
}
fn uuid(args: &Value, key: &str) -> Result<String> {
    Ok(uuid::Uuid::parse_str(text(args, key)?)
        .with_context(|| format!("{key} must be a UUID"))?
        .to_string())
}
fn validate_args(name: &str, args: &Value) -> Result<()> {
    let schema = description(name);
    let schema = &schema["inputSchema"];
    let map = args.as_object().context("arguments must be an object")?;
    for key in map.keys() {
        ensure!(
            schema["properties"].get(key).is_some(),
            "Unknown argument: {key}"
        );
    }
    for key in schema["required"].as_array().unwrap() {
        let key = key.as_str().unwrap();
        ensure!(map.contains_key(key), "Missing argument: {key}");
    }
    Ok(())
}
pub async fn invoke(
    api: &Api,
    name: &str,
    args: Value,
    mcp: bool,
    upload_root: Option<&crate::upload::UploadRoot>,
) -> Result<Value> {
    ensure!(NAMES.contains(&name), "Unknown tool");
    validate_args(name, &args)?;
    match name {
        "brands" | "channels" | "drafts" | "queue" | "history" | "health" => {
            let path = match name {
                "drafts" => "posts",
                "queue" | "history" => "publications",
                "health" => "jobs/health",
                other => other,
            };
            let query = if args.get("brand_id").is_some() {
                format!("?brand_id={}", uuid(&args, "brand_id")?)
            } else {
                String::new()
            };
            let mut response = api
                .request(Method::GET, &format!("/{path}{query}"), None)
                .await?;
            if matches!(name, "drafts" | "queue" | "history") {
                let values = response["data"]
                    .as_array_mut()
                    .context("Unexpected API list response")?;
                values.retain(|row| match name {
                    "drafts" => row["status"] == "draft",
                    "queue" => matches!(row["state"].as_str(), Some("queued" | "sending")),
                    _ => !matches!(row["state"].as_str(), Some("queued" | "sending")),
                });
            }
            Ok(response)
        }
        "post" | "attempts" => {
            let id = uuid(&args, "id")?;
            let path = if name == "post" {
                format!("/posts/{id}")
            } else {
                format!("/publications/{id}/attempts")
            };
            api.request(Method::GET, &path, None).await
        }
        "draft_create" | "draft_update" | "schedule" => {
            let body = args["body"].as_object().context("body must be an object")?;
            if name != "draft_create" {
                ensure!(
                    body.get("version")
                        .and_then(Value::as_i64)
                        .is_some_and(|v| v > 0),
                    "A positive expected version is required"
                );
            }
            let (method, path) = match name {
                "draft_create" => (Method::POST, "/posts".into()),
                "draft_update" => (Method::PATCH, format!("/posts/{}", uuid(&args, "id")?)),
                _ => {
                    match text(&args, "intent")? {
                        "queue" => ensure!(
                            !body.contains_key("scheduled_at"),
                            "queue intent must omit scheduled_at"
                        ),
                        "exact" => ensure!(
                            body.get("scheduled_at").and_then(Value::as_str).is_some(),
                            "exact intent requires scheduled_at with an explicit offset"
                        ),
                        _ => bail!("intent must be queue or exact"),
                    }
                    (
                        Method::POST,
                        format!("/posts/{}/schedule", uuid(&args, "id")?),
                    )
                }
            };
            api.request(method, &path, Some(Value::Object(body.clone())))
                .await
        }
        "media_upload" => {
            let brand = uuid(&args, "brand_id")?;
            let path = Path::new(text(&args, "path")?);
            let file = crate::upload::open(path, mcp, upload_root)?;
            api.upload(&brand, path, file).await
        }
        _ => unreachable!(),
    }
}
