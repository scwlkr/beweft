use crate::*;
fn segment(id: &str) -> String {
    url::form_urlencoded::byte_serialize(id.as_bytes()).collect()
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub value: i64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct InboxItem {
    pub id: String,
    pub parent_id: Option<String>,
    pub author: String,
    pub text: String,
    pub url: Option<String>,
    pub created_at: Option<String>,
    #[serde(default)]
    pub reply_context: Option<ReplyContext>,
}
fn metrics(v: &Value, keys: &[&str]) -> Vec<Metric> {
    keys.iter()
        .filter_map(|key| {
            v[*key]
                .as_i64()
                .or_else(|| v[*key].as_str().and_then(|s| s.parse().ok()))
                .map(|value| Metric {
                    name: (*key).into(),
                    value,
                })
        })
        .collect()
}
mod inbox;
mod metrics;
mod reply;

/// Preserved provider audience: never infer privacy from the replying account default.
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplyContext {
    pub visibility: String,
    pub recipients: Vec<ReplyRecipient>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplyRecipient {
    pub id: String,
    pub acct: String,
}
