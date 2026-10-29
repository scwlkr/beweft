use crate::contracts::*;
use crate::openapi_operations::OPERATIONS;
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
        PostSummary,
        Media,
        MediaLimits,
        ScheduleRule,
        PublicationState,
        SchedulingMode,
        PublicationPayload,
        Publication,
        PublicationSummary,
        ScheduleRequestIntent,
        ScheduleRequestResult,
        PublicationAttempt,
        JobHealth,
        WorkflowSummary,
        Analytics,
        Inbox,
        ProviderDescriptor,
        IdeaStatus,
        UpdateBrandRequest,
        CreateIdeaGroupRequest,
        UpdateIdeaGroupRequest,
        CreateTemplateRequest,
        UpdateTemplateRequest,
        ApplyTemplateRequest,
        UpdateMediaRequest,
        ReplacePublicationRequest,
        PrioritizeRequest,
        PasswordRequest,
        IdeaGroup,
        Template,
        TemplateStarter,
        ManualGuidance,
        MediaMetadata,
        ReplacedPublication,
        SessionInfo
    ))
)]
struct ApiDoc;
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
        let mut envelope =
            json!({"type":"object","required":["data"],"properties":{"data":response_schema}});
        if method == "get" && ["/posts", "/media", "/publications"].contains(&path) {
            let summaries = match path {
                "/posts" => Some("PostSummary"),
                "/publications" => Some("PublicationSummary"),
                _ => None,
            };
            let mut variants = Vec::new();
            for model in std::iter::once(response).chain(summaries) {
                let data = json!({"type":"array","items":reference(model)});
                variants
                    .push(json!({"type":"object","required":["data"],"properties":{"data":data}}));
                variants.push(json!({"type":"object","required":["data","next_cursor"],"properties":{"data":data,"next_cursor":{"type":["string","null"]}}}));
            }
            envelope = json!({"anyOf":variants});
        } else if method == "post" && path == "/posts/{id}/schedule" {
            envelope["properties"]["data"] = json!({"anyOf":[
                {"type":"array","items":reference("Publication")},
                {"type":"array","items":reference("PublicationSummary")}
            ]});
        }
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
            json!({"application/json":{"schema":envelope}})
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
        if path.contains("{request_id}") {
            parameters.push(json!({"name":"request_id","in":"path","required":true,"schema":{"type":"string","format":"uuid"}}));
        }
        if (method == "get"
            && [
                "/channels",
                "/ideas",
                "/idea-groups",
                "/templates",
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
        if method == "get" && ["/posts", "/media", "/publications"].contains(&path) {
            parameters.push(json!({"name":"limit","in":"query","required":false,"schema":{"type":"integer","minimum":1,"maximum":10}}));
            parameters.push(json!({"name":"cursor","in":"query","required":false,"schema":{"type":"string","maxLength":2048}}));
        }
        if method == "get" && path == "/posts" {
            parameters.push(json!({"name":"status","in":"query","required":false,"schema":{"type":"string","enum":["draft"]}}));
        }
        if method == "get" && path == "/publications" {
            parameters.push(json!({"name":"id","in":"query","required":false,"schema":{"type":"string","format":"uuid"}}));
            parameters.push(json!({"name":"group","in":"query","required":false,"schema":{"type":"string","enum":["queue","history"]}}));
        }
        if (method == "get" && ["/posts", "/publications"].contains(&path))
            || (method == "post" && path == "/posts/{id}/schedule")
        {
            parameters.push(json!({"name":"view","in":"query","required":false,"schema":{"type":"string","enum":["summary"]}}));
        }
        if method == "get" && path == "/brands" {
            parameters.push(json!({"name":"include_archived","in":"query","required":false,"schema":{"type":"boolean","default":false}}));
        }
        if method == "get" && path == "/ideas" {
            for (name, schema) in [
                ("group_id", json!({"type":"string","format":"uuid"})),
                ("status", reference("IdeaStatus")),
                ("tag", json!({"type":"string"})),
            ] {
                parameters.push(json!({"name":name,"in":"query","required":false,"schema":schema}));
            }
        }
        if method != "get" {
            parameters.push(json!({"name":"X-Beweft-Request","in":"header","required":false,"description":"Must equal 1 for cookie-authenticated mutations and bootstrap/login. Bearer requests do not require it.","schema":{"type":"string","enum":["1"]}}));
        }
        if !parameters.is_empty() {
            operation["parameters"] = json!(parameters);
        }
        if ["/auth/logout", "/auth/password", "/tokens", "/tokens/{id}"].contains(&path)
            || path.starts_with("/auth/sessions")
        {
            operation["security"] = json!([{"session":[]}]);
            operation["description"] = json!(
                "Requires an owner cookie session. API bearer tokens are rejected even with admin scope."
            );
        } else if !["/auth/status", "/auth/bootstrap", "/auth/login"].contains(&path) {
            operation["security"] = json!([{"session":[]},{"bearer":[]}]);
        }
        if !request.is_empty() {
            operation["requestBody"] = if request == "multipart" {
                json!({"required":true,"content":{"multipart/form-data":{"schema":{"type":"object","required":["file"],"additionalProperties":false,"properties":{"file":{"type":"string","format":"binary"}}}}}})
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
