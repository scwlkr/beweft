use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use utoipa::ToSchema;
use uuid::Uuid;
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BootstrapRequest {
    pub setup_secret: String,
    pub email: String,
    pub password: String,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TokenScope {
    Read,
    #[serde(rename = "drafts:write")]
    DraftsWrite,
    #[serde(rename = "publishing:write")]
    PublishingWrite,
    Admin,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTokenRequest {
    pub name: String,
    pub scopes: Vec<TokenScope>,
    #[serde(default)]
    pub brand_ids: Vec<Uuid>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateBrandRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateChannelRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_platform: Option<String>,
    pub brand_id: Uuid,
    pub name: String,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateChannelRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateIdeaRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<super::IdeaStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "super::present_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub group_id: Option<Option<Uuid>>,

    pub brand_id: Uuid,
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateIdeaRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<super::IdeaStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "super::present_nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub group_id: Option<Option<Uuid>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PostVariant {
    pub channel_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_ids: Option<Vec<Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<BTreeMap<String, Value>>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePostRequest {
    pub brand_id: Uuid,
    #[serde(default)]
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub media_ids: Vec<Uuid>,
    #[serde(default)]
    pub variants: Vec<PostVariant>,
    #[serde(default)]
    pub options: BTreeMap<String, Value>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePostRequest {
    pub version: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_ids: Option<Vec<Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<PostVariant>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<BTreeMap<String, Value>>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SchedulePostRequest {
    /// The revision displayed and approved by the caller. Conflicts if the locked draft has changed.
    pub version: i32,
    pub channel_ids: Vec<Uuid>,
    /// Omit to reserve the next posting-rule slot. Supplying a time creates a fixed-date publication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<DateTime<Utc>>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateScheduleRequest {
    pub channel_id: Uuid,
    #[schema(min_items = 1, max_items = 7)]
    pub days: Vec<u32>,
    #[schema(example = "09:00")]
    pub time: String,
    #[schema(example = "America/Chicago")]
    pub timezone: String,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReorderQueueRequest {
    pub channel_id: Uuid,
    /// Complete list of queued publications with scheduling_mode=queue. Exact-date publications are excluded and remain fixed.
    pub publication_ids: Vec<Uuid>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ManualCompleteRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Published,
    NotPublished,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReconcileRequest {
    pub resolution: Resolution,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RescheduleRequest {
    pub scheduled_at: DateTime<Utc>,
}
