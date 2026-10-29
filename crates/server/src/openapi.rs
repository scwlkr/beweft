use crate::contracts::*;
use axum::Json;
use serde_json::{Value, json};
#[derive(utoipa::OpenApi)]
#[openapi(
    info(
        title = "beweft API",
        version = "0.1.0",
        description = "Typed self-hosted publishing API. Cookie mutations require X-Beweft-Request: 1. API tokens require appropriate scopes and brand access."
    ),
    components(schemas(
        BootstrapRequest,
        LoginRequest,
        TokenScope,
        CreateTokenRequest,
        CreateBrandRequest,
        CreateChannelRequest,
        UpdateChannelRequest,
        CreateIdeaRequest,
        UpdateIdeaRequest,
        PostVariant,
        CreatePostRequest,
        UpdatePostRequest,
        SchedulePostRequest,
        CreateScheduleRequest,
        ReorderQueueRequest,
        ManualCompleteRequest,
        Resolution,
        ReconcileRequest,
        RescheduleRequest,
        AuthStatus,
        Owner,
        OkResponse,
        ErrorDetail,
        ErrorResponse,
        TokenMetadata,
        CreatedToken,
        Brand,
        Channel,
        Idea,
        PostStatus,
        Post,
        Media,
        MediaLimits,
        ScheduleRule,
        PublicationState,
        SchedulingMode,
        PublicationPayload,
        Publication,
        PublicationAttempt,
        JobHealth,
        WorkflowSummary,
        Analytics,
        Inbox,
        ProviderDescriptor
    ))
)]
struct ApiDoc;
// method, route, typed request (empty means no JSON body), typed response, list response.
const OPERATIONS: &[(&str, &str, &str, &str, bool)] = &[
    ("get", "/auth/status", "", "AuthStatus", false),
    (
        "post",
        "/auth/bootstrap",
        "BootstrapRequest",
        "Owner",
        false,
    ),
    ("post", "/auth/login", "LoginRequest", "Owner", false),
    ("get", "/auth/me", "", "Owner", false),
    ("post", "/auth/logout", "", "OkResponse", false),
    ("get", "/tokens", "", "TokenMetadata", true),
    (
        "post",
        "/tokens",
        "CreateTokenRequest",
        "CreatedToken",
        false,
    ),
    ("delete", "/tokens/{id}", "", "OkResponse", false),
    ("get", "/brands", "", "Brand", true),
    ("post", "/brands", "CreateBrandRequest", "Brand", false),
    ("get", "/providers", "", "ProviderDescriptor", true),
    ("get", "/channels", "", "Channel", true),
    (
        "post",
        "/channels",
        "CreateChannelRequest",
        "Channel",
        false,
    ),
    (
        "patch",
        "/channels/{id}",
        "UpdateChannelRequest",
        "Channel",
        false,
    ),
    ("get", "/ideas", "", "Idea", true),
    ("post", "/ideas", "CreateIdeaRequest", "Idea", false),
    ("patch", "/ideas/{id}", "UpdateIdeaRequest", "Idea", false),
    ("delete", "/ideas/{id}", "", "OkResponse", false),
    ("get", "/posts", "", "Post", true),
    ("post", "/posts", "CreatePostRequest", "Post", false),
    ("get", "/posts/{id}", "", "Post", false),
    ("patch", "/posts/{id}", "UpdatePostRequest", "Post", false),
    ("delete", "/posts/{id}", "", "OkResponse", false),
    (
        "post",
        "/posts/{id}/schedule",
        "SchedulePostRequest",
        "Publication",
        true,
    ),
    ("get", "/media", "", "Media", true),
    ("post", "/media", "multipart", "Media", false),
    ("get", "/media/limits", "", "MediaLimits", false),
    ("delete", "/media/{id}", "", "OkResponse", false),
    ("get", "/media/{id}/file", "", "binary", false),
    ("get", "/schedules", "", "ScheduleRule", true),
    (
        "post",
        "/schedules",
        "CreateScheduleRequest",
        "ScheduleRule",
        false,
    ),
    ("delete", "/schedules/{id}", "", "OkResponse", false),
    ("get", "/publications", "", "Publication", true),
    (
        "get",
        "/publications/{id}/attempts",
        "",
        "PublicationAttempt",
        true,
    ),
    (
        "post",
        "/publications/{id}/cancel",
        "",
        "Publication",
        false,
    ),
    ("post", "/publications/{id}/retry", "", "Publication", false),
    (
        "post",
        "/publications/{id}/manual-complete",
        "ManualCompleteRequest",
        "Publication",
        false,
    ),
    (
        "post",
        "/publications/{id}/reconcile",
        "ReconcileRequest",
        "Publication",
        false,
    ),
    (
        "post",
        "/publications/{id}/reschedule",
        "RescheduleRequest",
        "Publication",
        false,
    ),
    (
        "post",
        "/queue/reorder",
        "ReorderQueueRequest",
        "Publication",
        true,
    ),
    ("get", "/jobs/health", "", "JobHealth", false),
    ("get", "/analytics", "", "Analytics", false),
    ("get", "/inbox", "", "Inbox", false),
];
fn reference(name: &str) -> Value {
    json!({"$ref":format!("#/components/schemas/{name}")})
}
pub fn build_document() -> Value {
    use utoipa::OpenApi;
    let mut document = serde_json::to_value(ApiDoc::openapi()).expect("OpenAPI serialization");
    let mut paths = serde_json::Map::new();
    for &(method, path, request, response, list) in OPERATIONS {
        let response_schema = if list {
            json!({"type":"array","items":reference(response)})
        } else {
            reference(response)
        };
        let content = if response == "binary" {
            let mut media = serde_json::Map::new();
            for mime in [
                "image/jpeg",
                "image/png",
                "image/gif",
                "image/webp",
                "video/mp4",
                "video/webm",
            ] {
                media.insert(
                    mime.into(),
                    json!({"schema":{"type":"string","format":"binary"}}),
                );
            }
            Value::Object(media)
        } else {
            json!({"application/json":{"schema":{"type":"object","required":["data"],"properties":{"data":response_schema}}}})
        };
        let mut operation = json!({"operationId":format!("{}_{}",method,path.replace('/',"_").replace(['{','}'],"")),"responses":{"200":{"description":"Success","content":content}}});
        for (status, description) in [
            ("400", "Invalid input"),
            ("401", "Authentication required"),
            ("403", "Scope, brand or origin denied"),
            ("404", "Record not found"),
            ("409", "Version, state or slot conflict"),
            ("413", "Request exceeds configured limit"),
            ("422", "Typed request validation failed"),
            ("429", "Too many authentication attempts"),
            ("500", "Internal failure"),
        ] {
            operation["responses"][status] = json!({"description":description,"content":{"application/json":{"schema":reference("ErrorResponse")}}});
        }
        let mut parameters = Vec::new();
        if path.contains("{id}") {
            parameters.push(json!({"name":"id","in":"path","required":true,"schema":{"type":"string","format":"uuid"}}));
        }
        if (method == "get"
            && [
                "/channels",
                "/ideas",
                "/posts",
                "/media",
                "/publications",
                "/analytics",
                "/inbox",
            ]
            .contains(&path))
            || (method == "post" && path == "/media")
        {
            parameters.push(json!({"name":"brand_id","in":"query","required":method=="post","schema":{"type":"string","format":"uuid"}}));
        }
        if method == "get" && ["/schedules", "/publications"].contains(&path) {
            parameters.push(json!({"name":"channel_id","in":"query","required":path=="/schedules","schema":{"type":"string","format":"uuid"}}));
        }
        if method != "get" {
            parameters.push(json!({"name":"X-Beweft-Request","in":"header","required":false,"description":"Must equal 1 for cookie-authenticated mutations and bootstrap/login. Bearer requests do not require it.","schema":{"type":"string","enum":["1"]}}));
        }
        if !parameters.is_empty() {
            operation["parameters"] = json!(parameters);
        }
        if ["/auth/logout", "/tokens", "/tokens/{id}"].contains(&path) {
            operation["security"] = json!([{"session":[]}]);
            operation["description"] = json!(
                "Requires an owner cookie session. API bearer tokens are rejected even with admin scope."
            );
        } else if !["/auth/status", "/auth/bootstrap", "/auth/login"].contains(&path) {
            operation["security"] = json!([{"session":[]},{"bearer":[]}]);
        }
        if !request.is_empty() {
            operation["requestBody"] = if request == "multipart" {
                json!({"required":true,"content":{"multipart/form-data":{"schema":{"type":"object","required":["file"],"properties":{"file":{"type":"string","format":"binary"}}}}}})
            } else {
                json!({"required":true,"content":{"application/json":{"schema":reference(request)}}})
            };
        }
        let entry = paths
            .entry(format!("/api/v1{path}"))
            .or_insert_with(|| json!({}));
        entry[method] = operation;
    }
    document["paths"] = Value::Object(paths);
    document["components"]["securitySchemes"] = json!({"session":{"type":"apiKey","in":"cookie","name":"beweft_session"},"bearer":{"type":"http","scheme":"bearer"}});
    document
}
pub async fn document() -> Json<Value> {
    Json(build_document())
}
