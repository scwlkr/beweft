use crate::{
    auth::Auth,
    catalog::Filter,
    contracts::{ApplyTemplateRequest, CreateTemplateRequest, UpdateTemplateRequest},
    error::{ApiError, Result, data, required, uuid},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde_json::{Value, json};
use uuid::Uuid;
pub async fn starters(_auth: Auth) -> Json<Value> {
    data(json!([
     {"id":"announcement","name":"A useful announcement","title":"Introducing {{topic}}","text":"We made {{topic}} for people who {{need}}.\n\nHere is what it helps you do: {{benefit}}.\n\nExplore it here: {{link}}"},
     {"id":"behind-the-scenes","name":"A look behind the work","title":"Behind the scenes at {{brand}}","text":"A small detail from this week's work: {{detail}}.\n\nWe chose this approach because {{reason}}.\n\nWhat would you like to see next?"},
     {"id":"weekly-note","name":"The weekly note","title":"This week at {{brand}}","text":"This week we worked on {{focus}}.\n\nOne thing we learned: {{lesson}}.\n\nNext up: {{next}}."},
     {"id":"community-question","name":"A question worth asking","title":"Your perspective","text":"We are thinking about {{topic}}.\n\nWhat has your experience been with {{question}}?"}
    ]))
}
pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>> {
    if let Some(id) = filter.brand_id {
        auth.brand(id)?;
    }
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(t) FROM templates t WHERE ($1::uuid IS NULL OR brand_id=$1) AND (cardinality($2::uuid[])=0 OR brand_id=ANY($2)) ORDER BY updated_at DESC").bind(filter.brand_id).bind(&auth.brand_ids).fetch_all(&state.db).await?;
    Ok(data(rows))
}
fn name(body: &Value) -> Result<String> {
    let name = required(body, "name")?;
    if name.chars().count() > 120 {
        return Err(ApiError::bad(
            "Template name must be at most 120 characters",
        ));
    }
    Ok(name)
}
pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(input): Json<CreateTemplateRequest>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    auth.brand(input.brand_id)?;
    let body = serde_json::to_value(input).map_err(ApiError::internal)?;
    let brand = uuid(&body, "brand_id")?;
    crate::posts::validate_content(&state.db, brand, &body).await?;
    let row:Value=sqlx::query_scalar("INSERT INTO templates(id,brand_id,name,title,text,media_ids,variants,options) VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING to_jsonb(templates.*)").bind(Uuid::new_v4()).bind(brand).bind(name(&body)?).bind(body["title"].as_str().unwrap_or("")).bind(body["text"].as_str().unwrap_or("")).bind(&body["media_ids"]).bind(&body["variants"]).bind(&body["options"]).fetch_one(&state.db).await?;
    Ok(data(row))
}
pub async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateTemplateRequest>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let patch = serde_json::to_value(input).map_err(ApiError::internal)?;
    let mut body: Value = sqlx::query_scalar("SELECT to_jsonb(t) FROM templates t WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let brand = uuid(&body, "brand_id")?;
    auth.brand(brand)?;
    for key in ["name", "title", "text", "media_ids", "variants", "options"] {
        if let Some(value) = patch.get(key) {
            body[key] = value.clone();
        }
    }
    crate::posts::validate_content(&state.db, brand, &body).await?;
    let row:Option<Value>=sqlx::query_scalar("UPDATE templates SET name=$3,title=$4,text=$5,media_ids=$6,variants=$7,options=$8,version=version+1,updated_at=now() WHERE id=$1 AND version=$2 RETURNING to_jsonb(templates.*)").bind(id).bind(patch["version"].as_i64().unwrap() as i32).bind(name(&body)?).bind(body["title"].as_str().unwrap_or("")).bind(body["text"].as_str().unwrap_or("")).bind(&body["media_ids"]).bind(&body["variants"]).bind(&body["options"]).fetch_optional(&state.db).await?;
    Ok(data(row.ok_or_else(|| {
        ApiError::conflict("Template changed; reload before saving")
    })?))
}
pub async fn delete(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let brand: Uuid = sqlx::query_scalar("SELECT brand_id FROM templates WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    auth.brand(brand)?;
    sqlx::query("DELETE FROM templates WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(data(json!({"ok":true})))
}
fn substitute(
    text: &str,
    variables: &std::collections::BTreeMap<String, String>,
) -> Result<String> {
    // One pass: inserted values are never interpreted again as template syntax.
    let mut output = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let Some(end) = tail.find("}}") else {
            output.push_str(&rest[start..]);
            return Ok(output);
        };
        let key = tail[..end].trim();
        if let Some(value) = variables.get(key) {
            output.push_str(value);
        } else {
            output.push_str(&rest[start..start + end + 4]);
        }
        if output.len() > 100_000 {
            return Err(ApiError::bad(
                "Expanded template text exceeds 100,000 bytes",
            ));
        }
        rest = &tail[end + 2..];
    }
    output.push_str(rest);
    if output.len() > 100_000 {
        return Err(ApiError::bad(
            "Expanded template text exceeds 100,000 bytes",
        ));
    }
    Ok(output)
}
pub async fn apply(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(mut input): Json<ApplyTemplateRequest>,
) -> Result<Json<Value>> {
    auth.scope("drafts:write")?;
    let mut body: Value = sqlx::query_scalar("SELECT to_jsonb(t) FROM templates t WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let brand = uuid(&body, "brand_id")?;
    auth.brand(brand)?;
    if input.variables.len() > 50
        || input
            .variables
            .iter()
            .any(|(k, v)| k.len() > 80 || v.len() > 10_000)
    {
        return Err(ApiError::bad(
            "At most 50 template variables, names up to 80 bytes and values up to 10,000 bytes",
        ));
    }
    let brand_name: String = sqlx::query_scalar("SELECT name FROM brands WHERE id=$1")
        .bind(brand)
        .fetch_one(&state.db)
        .await?;
    input.variables.entry("brand".into()).or_insert(brand_name);
    for key in ["title", "text"] {
        body[key] = json!(substitute(
            body[key].as_str().unwrap_or(""),
            &input.variables
        )?);
    }
    if let Some(variants) = body["variants"].as_array_mut() {
        for variant in variants {
            if let Some(text) = variant["text"].as_str() {
                variant["text"] = json!(substitute(text, &input.variables)?);
            }
        }
    }
    let create = json!({"brand_id":brand,"title":body["title"],"text":body["text"],"media_ids":body["media_ids"],"variants":body["variants"],"options":body["options"]});
    let input = serde_json::from_value(create).map_err(ApiError::internal)?;
    crate::posts::create(State(state), auth, Json(input)).await
}
