use axum::Json;
use serde_json::{Value, json};
#[derive(utoipa::OpenApi)]
#[openapi(info(
    title = "beweft API",
    version = "0.1.0",
    description = "Self-hosted publishing API. State-changing cookie requests require X-Beweft-Request: 1. See docs/api-contract.md for field descriptions."
))]
struct ApiDoc;
/// Build the public operation inventory on the same shared JSON types accepted by handlers.
pub async fn document() -> Json<Value> {
    use utoipa::OpenApi;
    let mut document = serde_json::to_value(ApiDoc::openapi()).expect("OpenAPI serialization");
    let mut paths = serde_json::Map::new();
    for (path, methods) in [
        ("/auth/status", "get"),
        ("/auth/bootstrap", "post"),
        ("/auth/login", "post"),
        ("/auth/me", "get"),
        ("/auth/logout", "post"),
        ("/tokens", "get post"),
        ("/tokens/{id}", "delete"),
        ("/brands", "get post"),
        ("/providers", "get"),
        ("/channels", "get post"),
        ("/channels/{id}", "patch"),
        ("/ideas", "get post"),
        ("/ideas/{id}", "patch delete"),
        ("/posts", "get post"),
        ("/posts/{id}", "get patch delete"),
        ("/posts/{id}/schedule", "post"),
        ("/media", "get post"),
        ("/media/{id}", "delete"),
        ("/media/{id}/file", "get"),
        ("/schedules", "get post"),
        ("/schedules/{id}", "delete"),
        ("/publications", "get"),
        ("/publications/{id}/attempts", "get"),
        ("/publications/{id}/cancel", "post"),
        ("/publications/{id}/retry", "post"),
        ("/publications/{id}/manual-complete", "post"),
        ("/publications/{id}/reconcile", "post"),
        ("/publications/{id}/reschedule", "post"),
        ("/queue/reorder", "post"),
        ("/jobs/health", "get"),
        ("/analytics", "get"),
        ("/inbox", "get"),
    ] {
        let mut operations = serde_json::Map::new();
        for method in methods.split_whitespace() {
            let mut operation = json!({"operationId":format!("{}_{}",method,path.replace('/',"_").replace(['{','}'],"")),"responses":{"200":{"description":"Success","content":{"application/json":{"schema":{"type":"object","required":["data"],"properties":{"data":{}}}}}},"400":{"description":"Validation failure"},"401":{"description":"Authentication required"},"403":{"description":"Insufficient scope or brand access"},"409":{"description":"State or version conflict"}}});
            if path.contains("{id}") {
                operation["parameters"] = json!([{"name":"id","in":"path","required":true,"schema":{"type":"string","format":"uuid"}}]);
            }
            if !["/auth/status", "/auth/bootstrap", "/auth/login"].contains(&path) {
                operation["security"] = json!([{"session":[]},{"bearer":[]}]);
            }
            if ["post", "patch"].contains(&method) {
                operation["requestBody"] = if path == "/media" {
                    json!({"required":true,"content":{"multipart/form-data":{"schema":{"type":"object","required":["file"],"properties":{"file":{"type":"string","format":"binary"}}}}}})
                } else {
                    json!({"content":{"application/json":{"schema":{"type":"object","additionalProperties":true}}}})
                };
            }
            operations.insert(method.into(), operation);
        }
        paths.insert(format!("/api/v1{path}"), Value::Object(operations));
    }
    document["paths"] = Value::Object(paths);
    document["components"] = json!({"securitySchemes":{"session":{"type":"apiKey","in":"cookie","name":"beweft_session"},"bearer":{"type":"http","scheme":"bearer"}}});
    Json(document)
}
