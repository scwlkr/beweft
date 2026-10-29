use super::{PostVariant, TokenScope};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use utoipa::ToSchema;
use uuid::Uuid;
#[derive(Serialize, Deserialize, ToSchema)]
pub struct AuthStatus {
    pub initialized: bool,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Owner {
    pub id: Uuid,
    pub email: String,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct OkResponse {
    pub ok: bool,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct TokenMetadata {
    pub id: Uuid,
    pub name: String,
    pub scopes: Vec<TokenScope>,
    pub brand_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct CreatedToken {
    pub id: Uuid,
    pub token: String,
    pub name: String,
    pub scopes: Vec<TokenScope>,
    pub brand_ids: Vec<Uuid>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Brand {
    pub id: Uuid,
    pub name: String,
    pub color: String,
    pub created_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Channel {
    pub id: Uuid,
    pub brand_id: Uuid,
    pub name: String,
    pub provider: String,
    pub timezone: String,
    pub paused: bool,
    pub capabilities: BTreeMap<String, Value>,
    pub created_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Idea {
    pub id: Uuid,
    pub brand_id: Uuid,
    pub title: String,
    pub text: String,
    pub color: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PostStatus {
    Draft,
    Scheduled,
    Published,
    Attention,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Post {
    pub id: Uuid,
    pub brand_id: Uuid,
    pub title: String,
    pub text: String,
    pub media_ids: Vec<Uuid>,
    pub variants: Vec<PostVariant>,
    pub options: BTreeMap<String, Value>,
    pub version: i32,
    pub status: PostStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Media {
    pub id: Uuid,
    pub brand_id: Uuid,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub url: String,
    pub created_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct MediaLimits {
    pub max_upload_bytes: u64,
    pub quota_bytes: u64,
    pub used_bytes: u64,
    pub reserved_bytes: u64,
    pub available_bytes: u64,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ScheduleRule {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub days: Vec<u32>,
    pub time: String,
    pub timezone: String,
    pub created_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PublicationState {
    Queued,
    Sending,
    ManualRequired,
    Published,
    FixtureComplete,
    Failed,
    Uncertain,
    Canceled,
}
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SchedulingMode {
    Queue,
    Exact,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct PublicationPayload {
    pub title: String,
    pub text: String,
    pub media_ids: Vec<Uuid>,
    pub options: BTreeMap<String, Value>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Publication {
    pub id: Uuid,
    pub post_id: Uuid,
    pub brand_id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub provider: String,
    pub post_version: i32,
    pub scheduled_at: DateTime<Utc>,
    pub scheduling_mode: SchedulingMode,
    pub state: PublicationState,
    pub payload: PublicationPayload,
    #[schema(required = true)]
    pub external_id: Option<String>,
    #[schema(required = true)]
    pub external_url: Option<String>,
    #[schema(required = true)]
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct PublicationAttempt {
    pub id: Uuid,
    pub publication_id: Uuid,
    pub state: PublicationState,
    pub provider: String,
    pub started_at: DateTime<Utc>,
    #[schema(required = true)]
    pub finished_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub receipt: Option<Value>,
    #[schema(required = true)]
    pub error: Option<String>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct JobHealth {
    #[schema(required = true)]
    pub worker_last_seen: Option<DateTime<Utc>>,
    pub worker_healthy: bool,
    pub queued: i64,
    pub uncertain: i64,
    pub manual_required: i64,
    #[schema(required = true)]
    pub oldest_due: Option<DateTime<Utc>>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct WorkflowSummary {
    pub scheduled: i64,
    pub published: i64,
    pub attention: i64,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Analytics {
    pub availability: String,
    pub reason: String,
    pub summary: WorkflowSummary,
    pub metrics: Vec<Value>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Inbox {
    pub availability: String,
    pub reason: String,
    pub items: Vec<Value>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ProviderDescriptor {
    pub id: String,
    pub name: String,
    pub implementation: String,
    pub configuration: String,
    pub execution: String,
    pub verification: String,
    pub formats: Vec<String>,
    pub reason: String,
}
