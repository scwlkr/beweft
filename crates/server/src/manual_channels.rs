use crate::{
    auth::Auth,
    error::{ApiError, Result, data},
};
use axum::Json;
use serde_json::{Value, json};
const PLATFORMS: &[(&str, &str, Option<&str>, &str)] = &[
    (
        "generic",
        "Other platform",
        None,
        "Copy the prepared text and download the media. Open your platform, review the post, publish it, then record its link here.",
    ),
    (
        "x",
        "X",
        Some("https://x.com/compose/post"),
        "Copy the text and add the prepared media in X. Review the audience and any image descriptions, publish, then record the post link.",
    ),
    (
        "youtube",
        "YouTube",
        Some("https://studio.youtube.com"),
        "Upload the prepared video in YouTube Studio. Review title, description, audience and visibility, finish publishing, then record the video link.",
    ),
    (
        "facebook",
        "Facebook",
        Some("https://www.facebook.com"),
        "Choose the intended Page or profile, paste the text and add media. Review the audience and publish, then record the post link.",
    ),
    (
        "instagram",
        "Instagram",
        Some("https://www.instagram.com"),
        "Open Instagram's create flow, upload the prepared media and paste the caption. Review crop, accessibility text and audience, publish, then record the link.",
    ),
    (
        "threads",
        "Threads",
        Some("https://www.threads.com"),
        "Create a thread, paste the prepared text and attach media. Review reply permissions, publish, then record the thread link.",
    ),
    (
        "tiktok",
        "TikTok",
        Some("https://www.tiktok.com/upload"),
        "Upload the prepared video in TikTok. Review caption, visibility, disclosures and interaction controls, publish, then record the video link.",
    ),
    (
        "bluesky",
        "Bluesky",
        Some("https://bsky.app"),
        "Create a post, paste the text, add media and image descriptions, review, publish, then record the post link.",
    ),
    (
        "mastodon",
        "Mastodon",
        None,
        "Open your own Mastodon server. Paste the text, attach media with descriptions, review visibility, publish, then record the post link.",
    ),
    (
        "linkedin",
        "LinkedIn",
        Some("https://www.linkedin.com/feed/"),
        "Select the intended profile or organization. Paste the text, attach media, review, publish, then record the post link.",
    ),
    (
        "pinterest",
        "Pinterest",
        Some("https://www.pinterest.com/pin-creation-tool/"),
        "Create a Pin with the prepared image or video. Choose the board, review title and destination link, publish, then record the Pin link.",
    ),
];
pub fn guidance(id: &str) -> Result<Value> {
    PLATFORMS
        .iter()
        .find(|p| p.0 == id)
        .map(|p| json!({"id":p.0,"name":p.1,"compose_url":p.2,"guidance":p.3}))
        .ok_or_else(|| ApiError::bad("Unknown manual target_platform"))
}
pub fn decorate(mut channel: Value) -> Value {
    // Stored grants and health remain channel-specific; implemented formats
    // come from this running version, including for channels connected earlier.
    if let Some(provider) = channel["provider"].as_str().map(str::to_owned)
        && matches!(provider.as_str(), "facebook" | "instagram" | "threads")
        && let Some(current) = beweft_providers::capabilities()
            .into_iter()
            .find(|c| c.id == provider)
        && channel["capabilities"].is_object()
    {
        channel["capabilities"]["formats"] = json!(current.formats);
        channel["capabilities"]["reason"] = json!(current.reason);
        if provider == "facebook" && channel["capabilities"]["permissions"].is_object() {
            let permissions = &channel["capabilities"]["permissions"];
            let scopes = permissions["reported_scopes"].as_array();
            let known = permissions["source"] == "provider_token_response" && scopes.is_some();
            let required = ["pages_manage_posts", "pages_read_engagement"];
            let missing: Vec<_> = required
                .iter()
                .copied()
                .filter(|required| {
                    !scopes.is_some_and(|scopes| scopes.iter().any(|scope| scope == *required))
                })
                .collect();
            let status = if !known {
                "unknown"
            } else if missing.is_empty() {
                "granted"
            } else {
                "missing"
            };
            channel["capabilities"]["permissions"]["actions"]["reel"] = json!({
                "status":status, "required_scopes":required,
                "missing_scopes":if known { json!(missing) } else { Value::Null }
            });
        }
    }
    channel["manual_guidance"] = if channel["provider"] == "manual" {
        guidance(channel["target_platform"].as_str().unwrap_or("generic")).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    channel
}
pub async fn list(_auth: Auth) -> Json<Value> {
    data(
        PLATFORMS
            .iter()
            .map(|p| guidance(p.0).unwrap())
            .collect::<Vec<_>>(),
    )
}
