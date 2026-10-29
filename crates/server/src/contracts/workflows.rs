use super::PostVariant;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use utoipa::ToSchema;
use uuid::Uuid;
pub fn present_nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IdeaStatus {
    Inbox,
    InProgress,
    Ready,
    Archived,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateBrandRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateIdeaGroupRequest {
    pub brand_id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub position: i32,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateIdeaGroupRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i32>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTemplateRequest {
    pub brand_id: Uuid,
    pub name: String,
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
pub struct UpdateTemplateRequest {
    pub version: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
pub struct ApplyTemplateRequest {
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMediaRequest {
    pub alt_text: String,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplacePublicationRequest {
    pub post_version: i32,
    pub publication_post_version: i32,
    pub scheduled_at: DateTime<Utc>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PrioritizeRequest {
    pub scheduled_at: DateTime<Utc>,
}
#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PasswordRequest {
    pub current_password: String,
    pub new_password: String,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct IdeaGroup {
    pub id: Uuid,
    pub brand_id: Uuid,
    pub name: String,
    pub color: String,
    pub position: i32,
    pub created_at: DateTime<Utc>,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct Template {
    pub id: Uuid,
    pub brand_id: Uuid,
    pub name: String,
    pub title: String,
    pub text: String,
    pub media_ids: Vec<Uuid>,
    pub variants: Vec<PostVariant>,
    pub options: BTreeMap<String, Value>,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct TemplateStarter {
    pub id: String,
    pub name: String,
    pub title: String,
    pub text: String,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct ManualGuidance {
    pub id: String,
    pub name: String,
    #[schema(required = true)]
    pub compose_url: Option<String>,
    pub guidance: String,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct MediaMetadata {
    pub id: Uuid,
    pub alt_text: String,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct ReplacedPublication {
    pub previous_publication_id: Uuid,
    pub publication: super::Publication,
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct SessionInfo {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub current: bool,
}
