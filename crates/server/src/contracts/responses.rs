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
    #[schema(required = true)]
    pub archived_at: Option<DateTime<Utc>>,
    pub id: Uuid,
    pub name: String,
    pub color: String,
    pub created_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Channel {
    #[schema(required = true)]
    pub target_platform: Option<String>,
    #[schema(required = true)]
    pub manual_guidance: Option<super::ManualGuidance>,
    #[schema(required = true)]
    pub connection_identity: Option<String>,
    pub connection_status: String,
    #[schema(required = true)]
    pub connection_error: Option<String>,
    #[schema(required = true)]
    pub connected_at: Option<DateTime<Utc>>,
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
    pub status: super::IdeaStatus,
    #[schema(required = true)]
    pub group_id: Option<Uuid>,
    pub tags: Vec<String>,
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
pub struct PostSummary {
    pub id: Uuid,
    pub brand_id: Uuid,
    pub title: String,
    pub text: String,
    pub media_ids: Vec<Uuid>,
    pub version: i32,
    pub status: PostStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Media {
    pub alt_text: String,
    pub metadata_version: i32,
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
    #[serde(default)]
    pub media_metadata: Vec<super::MediaMetadata>,
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
pub struct PublicationSummary {
    pub id: Uuid,
    pub post_id: Uuid,
    pub brand_id: Uuid,
    pub channel_id: Uuid,
    /// Display name truncated to at most 100 Unicode characters in summaries.
    #[schema(max_length = 100)]
    pub channel_name: String,
    /// Provider label truncated to at most 64 Unicode characters in summaries.
    #[schema(max_length = 64)]
    pub provider: String,
    pub post_version: i32,
    pub scheduled_at: DateTime<Utc>,
    pub scheduling_mode: SchedulingMode,
    pub state: PublicationState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ScheduleRequestIntent {
    pub version: i32,
    pub channel_ids: Vec<Uuid>,
    /// UTC RFC3339, rendered to microsecond precision; null means a posting-rule slot.
    #[schema(required = true)]
    pub scheduled_at: Option<DateTime<Utc>>,
    /// Original normalized media-version map, or null for legacy requests.
    #[schema(required = true)]
    pub media_versions: Option<BTreeMap<Uuid, i64>>,
}
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleRequestOutcome {
    Accepted,
    Rejected,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ScheduleRequestResult {
    pub request_id: Uuid,
    pub post_id: Uuid,
    pub brand_id: Uuid,
    pub request: ScheduleRequestIntent,
    pub outcome: ScheduleRequestOutcome,
    #[schema(required = true)]
    pub error: Option<ErrorDetail>,
    pub publications: Vec<PublicationSummary>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct PublicationAttempt {
    pub id: Uuid,
    pub publication_id: Uuid,
    pub state: PublicationAttemptState,
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
#[serde(rename_all = "snake_case")]
pub enum PublicationAttemptState {
    Sending,
    ManualRequired,
    Published,
    FixtureComplete,
    Failed,
    Uncertain,
    Reconciled,
    Continued,
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
    pub oauth: bool,
    pub analytics: bool,
    pub inbox: bool,
    pub reply: bool,
    pub reason: String,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ProviderApp {
    pub provider: String,
    pub client_id: String,
    #[schema(required = true)]
    pub instance_url: Option<String>,
    pub configured: bool,
    pub updated_at: DateTime<Utc>,
    #[schema(required = true)]
    pub revocation_id: Option<Uuid>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct SavedProviderApp {
    pub provider: String,
    pub client_id: String,
    #[schema(required = true)]
    pub instance_url: Option<String>,
    pub configured: bool,
    pub callback_url: String,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct StartedConnection {
    pub authorization_url: String,
    pub flow_id: Uuid,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ConnectionDestination {
    pub id: String,
    pub name: String,
    #[schema(required = true)]
    pub username: Option<String>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ConnectionFlow {
    pub id: Uuid,
    pub brand_id: Uuid,
    pub provider: String,
    pub destinations: Vec<ConnectionDestination>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct DisconnectResult {
    pub disconnected: bool,
    pub provider_revoked: bool,
    pub channels_requiring_reconnection: u64,
    #[schema(required = true)]
    pub revocation_id: Option<Uuid>,
    pub next_action: String,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct RevocationAcknowledged {
    pub revocation_acknowledged: bool,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ProviderRemovalStatus {
    pub provider: String,
    #[schema(required = true)]
    pub revocation_id: Option<Uuid>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ProviderSyncResult {
    pub metrics_observed: u64,
    pub inbox_items: u64,
    pub coverage: String,
    pub errors: Vec<String>,
    pub note: String,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ProviderReply {
    pub id: Uuid,
    pub inbox_id: Uuid,
    pub text: String,
    pub state: String,
    pub revision: i64,
    #[schema(required = true)]
    pub receipt: Option<Value>,
    #[schema(required = true)]
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ProviderStatus {
    pub session_expired: bool,
    pub reset_allowed: bool,
    #[schema(required = true)]
    pub reason: Option<String>,
    #[schema(required = true)]
    pub x_upload: Option<XUploadStatus>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct XUploadStatus {
    /// `post_started` is the public delivery boundary, including text-only posts.
    pub phase: XUploadPhase,
    pub attempt_id: Uuid,
    pub checkpoint_id: Uuid,
    pub reset_allowed: bool,
    pub reason: String,
}
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum XUploadPhase {
    Upload,
    PostStarted,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ProviderReconcilePending {
    pub state: String,
    pub reason: String,
    pub session_expired: Option<bool>,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ResetExpiredSessionResult {
    pub state: String,
    pub session_reset: bool,
    pub next_action: String,
}
