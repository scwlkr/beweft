use crate::client::Api;
use anyhow::{Context, Result, bail, ensure};
use reqwest::Method;
use serde_json::{Value, json};
use std::{path::Path, sync::OnceLock};

pub const NAMES: &[&str] = &[
    "brands",
    "channels",
    "drafts",
    "post",
    "media",
    "queue",
    "history",
    "publication",
    "attempts",
    "health",
    "draft_create",
    "draft_update",
    "schedule",
    "schedule_request",
    "schedule_retire",
    "schedules",
    "media_upload",
];
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn contract() -> &'static Value {
    static CONTRACT: OnceLock<Value> = OnceLock::new();
    CONTRACT.get_or_init(|| {
        serde_json::from_str(include_str!("../../../docs/openapi.json"))
            .expect("generated API contract")
    })
}
fn parameter_schema(path: &str, method: &str, name: &str) -> Value {
    contract()["paths"][path][method]["parameters"]
        .as_array()
        .expect("generated API parameters")
        .iter()
        .find(|parameter| parameter["name"] == name)
        .expect("generated API parameter")["schema"]
        .clone()
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
    let spec = contract();
    expand(
        &spec["components"]["schemas"][name],
        &spec["components"]["schemas"],
        0,
    )
}
// Named scheduling tools deliberately require recovery fields that are optional
// for legacy API clients. The remaining fields come from the generated contract.
fn recovery_schema(name: &str) -> Value {
    let mut schema = request_schema(name);
    let fields: &[&str] = if name == "SchedulePostRequest" {
        &["request_id", "media_versions"]
    } else {
        &["media_versions"]
    };
    for field in fields {
        let required = schema["required"].as_array_mut().unwrap();
        if !required.contains(&json!(field)) {
            required.push(json!(field));
        }
    }
    schema["properties"]["media_versions"]["type"] = json!("object");
    schema["properties"]["media_versions"]["description"] = json!(
        "Explicit saved map of every selected effective media ID to its metadata version; use {} when there is no effective media."
    );
    if name == "SchedulePostRequest" {
        schema["properties"]["request_id"]["type"] = json!("string");
        schema["properties"]["request_id"]["description"] = json!(
            "Explicit UUID saved with the exact approved body before sending. Reuse it only with that body."
        );
    }
    schema
}
pub fn description(name: &str) -> Value {
    let id = json!({"type":"string","format":"uuid"});
    let filter = object(json!({"brand_id":id}), &[]);
    let paged = object(
        json!({"brand_id":id,"limit":{"type":"integer","minimum":1,"maximum":10,"default":10},"cursor":{"type":"string","maxLength":2048}}),
        &[],
    );
    let (summary, schema, read) = match name {
        "brands" => (
            "List brands accessible to this token.",
            object(json!({}), &[]),
            true,
        ),
        "channels" => ("List channels and connection health.", filter, true),
        "drafts" => (
            "Read a page of draft summaries (variants and options omitted); use post for complete content. Follow next_cursor for more.",
            paged,
            true,
        ),
        "post" => (
            "Read one post including its revision and channel variants.",
            object(json!({"id":id}), &["id"]),
            true,
        ),
        "media" => (
            "Read a page of media records, including IDs and file URLs. Follow next_cursor for more; use after an unknown upload outcome.",
            paged,
            true,
        ),
        "queue" => (
            "Read a page of queued and sending publication summaries, newest created first; use publication for full immutable content. Follow next_cursor for more.",
            paged,
            true,
        ),
        "history" => (
            "Read a page of completed, failed, manual, uncertain and canceled publication summaries; use publication for details. Follow next_cursor for more.",
            paged,
            true,
        ),
        "publication" => (
            "Read one publication including its immutable content, external receipt fields and error detail.",
            object(json!({"id":id}), &["id"]),
            true,
        ),
        "attempts" => (
            "Read durable delivery attempt history for a publication.",
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
            "Schedule the approved post version. Save the explicit request_id and exact body first; supply the complete effective media_versions map ({} if empty). intent=queue reserves posting-rule slots; intent=exact requires scheduled_at. Can publish when due. After an unknown outcome, look up schedule_request and preserve the same ID/body; a 404 or in-progress result is unresolved. Accepted receipts identify the original publications.",
            object(
                json!({"id":parameter_schema("/api/v1/posts/{id}/schedule", "post", "id"),"intent":{"type":"string","enum":["queue","exact"]},"body":recovery_schema("SchedulePostRequest")}),
                &["id", "intent", "body"],
            ),
            false,
        ),
        "schedule_request" => (
            "Read a saved scheduling receipt by request_id. Accepted identifies the original publications; rejected identifies a terminal rejection. A 404 may mean missing or inaccessible and cannot rule out an in-flight write.",
            object(
                json!({"request_id":parameter_schema("/api/v1/schedule-requests/{request_id}", "get", "request_id")}),
                &["request_id"],
            ),
            true,
        ),
        "schedule_retire" => (
            "Explicitly retire an unresolved saved scheduling request using the same request_id, post, brand, version, channels, time and media_versions. A retired receipt has no publications; an already accepted receipt is returned unchanged and its publications are not canceled. In-progress and missing outcomes remain unresolved.",
            object(
                json!({"request_id":parameter_schema("/api/v1/schedule-requests/{request_id}/retire", "post", "request_id"),"body":recovery_schema("RetireScheduleRequest")}),
                &["request_id", "body"],
            ),
            false,
        ),
        "schedules" => (
            "Read posting rules for a channel before choosing a queue schedule.",
            object(
                json!({"channel_id":parameter_schema("/api/v1/schedules", "get", "channel_id")}),
                &["channel_id"],
            ),
            true,
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
        "brands" | "channels" | "health" => {
            let path = match name {
                "health" => "jobs/health",
                other => other,
            };
            let query = if args.get("brand_id").is_some() {
                format!("?brand_id={}", uuid(&args, "brand_id")?)
            } else {
                String::new()
            };
            api.request(Method::GET, &format!("/{path}{query}"), None)
                .await
        }
        "drafts" | "media" | "queue" | "history" => {
            let limit = match args.get("limit") {
                None => 10,
                Some(value) => value
                    .as_u64()
                    .filter(|n| (1..=10).contains(n))
                    .context("limit must be between 1 and 10")?,
            };
            let mut query = format!("limit={limit}");
            if args.get("brand_id").is_some() {
                query.push_str(&format!("&brand_id={}", uuid(&args, "brand_id")?));
            }
            if let Some(cursor) = args.get("cursor") {
                let cursor = cursor.as_str().context("cursor must be a string")?;
                ensure!(
                    !cursor.is_empty()
                        && cursor.len() <= 2048
                        && cursor
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
                    "Invalid cursor"
                );
                query.push_str(&format!("&cursor={cursor}"));
            }
            let path = match name {
                "drafts" => {
                    query.push_str("&status=draft&view=summary");
                    "posts"
                }
                "queue" => {
                    query.push_str("&group=queue&view=summary");
                    "publications"
                }
                "history" => {
                    query.push_str("&group=history&view=summary");
                    "publications"
                }
                _ => "media",
            };
            api.request(Method::GET, &format!("/{path}?{query}"), None)
                .await
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
        "schedules" => {
            let channel_id = uuid(&args, "channel_id")?;
            api.request(
                Method::GET,
                &format!("/schedules?channel_id={channel_id}"),
                None,
            )
            .await
        }
        "schedule_request" => {
            let request_id = uuid(&args, "request_id")?;
            api.request(Method::GET, &format!("/schedule-requests/{request_id}"), None)
                .await
                .map_err(|error| anyhow::anyhow!("{error}. A missing or inaccessible receipt does not prove an in-flight request failed; keep the saved ID and body."))
        }
        "publication" => {
            let id = uuid(&args, "id")?;
            let response = api
                .request(Method::GET, &format!("/publications?limit=1&id={id}"), None)
                .await?;
            let record = response["data"]
                .as_array()
                .and_then(|rows| rows.first())
                .context("Publication not found or inaccessible")?;
            Ok(json!({"data":record}))
        }
        "draft_create" | "draft_update" | "schedule" | "schedule_retire" => {
            let body = args["body"].as_object().context("body must be an object")?;
            if name != "draft_create" {
                ensure!(
                    body.get("version")
                        .and_then(Value::as_i64)
                        .is_some_and(|v| v > 0),
                    "A positive expected version is required"
                );
            }
            if matches!(name, "schedule" | "schedule_retire") {
                ensure!(
                    body.get("media_versions").is_some_and(Value::is_object),
                    "Explicit media_versions object is required; use {{}} for no effective media"
                );
                if name == "schedule" {
                    uuid(&args["body"], "request_id")?;
                }
            }
            let (method, path) = match name {
                "draft_create" => (Method::POST, "/posts".into()),
                "draft_update" => (Method::PATCH, format!("/posts/{}", uuid(&args, "id")?)),
                "schedule_retire" => (
                    Method::POST,
                    format!("/schedule-requests/{}/retire", uuid(&args, "request_id")?),
                ),
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
                        format!("/posts/{}/schedule?view=summary", uuid(&args, "id")?),
                    )
                }
            };
            let response = api
                .request(method, &path, Some(Value::Object(body.clone())))
                .await;
            if matches!(name, "schedule" | "schedule_retire") {
                response.map_err(|error| anyhow::anyhow!("{error}. For an unresolved outcome, look up schedule_request and keep the same saved ID and body; a 404 or in-progress result is not proof of failure."))
            } else {
                response
            }
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
