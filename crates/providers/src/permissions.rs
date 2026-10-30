use crate::*;
fn required_scopes(provider: &str, action: &str) -> Vec<&'static str> {
    match (provider, action) {
        ("x", "publish") => vec!["tweet.write"],
        ("x", "media") => vec!["tweet.write", "media.write"],
        ("x", "metrics") => vec!["tweet.read"],
        ("youtube", "publish" | "media") => vec!["https://www.googleapis.com/auth/youtube.upload"],
        ("youtube", "reply" | "inbox") => vec!["https://www.googleapis.com/auth/youtube.force-ssl"],
        ("youtube", "metrics") => vec!["https://www.googleapis.com/auth/youtube.readonly"],
        ("google_business", "publish" | "media") => {
            vec!["https://www.googleapis.com/auth/business.manage"]
        }
        ("facebook", "publish" | "media") => vec!["pages_manage_posts"],
        ("facebook", "reel") => vec!["pages_manage_posts", "pages_read_engagement"],
        ("facebook", "video") => vec![
            "pages_manage_posts",
            "pages_read_engagement",
            "pages_show_list",
        ],
        ("facebook", "reply") => vec!["pages_manage_engagement"],
        ("facebook", "inbox") => vec!["pages_read_engagement"],
        ("facebook", "metrics") => vec!["pages_read_engagement"],
        ("instagram", "publish" | "media") => vec!["instagram_business_content_publish"],
        ("instagram", "story") => vec![
            "instagram_business_basic",
            "instagram_business_content_publish",
        ],
        ("instagram", "inbox" | "reply") => vec!["instagram_business_manage_comments"],
        ("instagram", "metrics") => vec!["instagram_business_basic"],
        ("threads", "publish" | "media") => vec!["threads_content_publish"],
        ("threads", "inbox") => vec!["threads_read_replies"],
        ("threads", "reply") => vec!["threads_manage_replies"],
        ("threads", "metrics") => vec!["threads_manage_insights"],
        ("mastodon", "publish" | "media" | "reply") => vec!["write"],
        ("mastodon", "metrics" | "inbox") => vec!["read"],
        ("linkedin", "publish" | "media") => vec!["w_member_social"],
        ("pinterest", "publish" | "media") => vec!["pins:write"],
        _ => vec![],
    }
}
pub fn grant_permissions(c: &Channel) -> Value {
    let mut actions = serde_json::Map::new();
    let mut supported_actions = vec!["publish", "media", "metrics", "inbox", "reply"];
    if c.app.provider == "facebook" {
        supported_actions.push("reel");
        supported_actions.push("video");
    }
    if c.app.provider == "instagram" {
        supported_actions.push("story");
    }
    for action in supported_actions {
        actions.insert(
            action.into(),
            action_permission(
                &c.app.provider,
                action,
                &c.tokens.scopes,
                c.tokens.scopes_known,
            ),
        );
    }
    json!({"source":if c.tokens.scopes_known {"provider_token_response"}else{"provider_did_not_report_scopes"},"reported_scopes":c.tokens.scopes,"actions":actions,"note":"Granted scopes do not prove format eligibility, provider app review, account access or successful publication."})
}
/// Reconstruct an action from already-reported scopes without reading or changing credentials.
pub fn action_permission(provider: &str, action: &str, scopes: &[String], known: bool) -> Value {
    let required = required_scopes(provider, action);
    let missing: Vec<_> = required
        .iter()
        .filter(|s| !scopes.iter().any(|g| g == **s))
        .collect();
    let status = if !known || required.is_empty() {
        "unknown"
    } else if missing.is_empty() {
        "granted"
    } else {
        "missing"
    };
    json!({"status":status,"required_scopes":required,"missing_scopes":if known {json!(missing)} else {Value::Null}})
}
pub fn require_permission(c: &Channel, action: &str) -> Result<()> {
    if grant_permissions(c)["actions"][action]["status"] == "missing" {
        return Err(error(
            FailureKind::Authentication,
            "The provider grant is missing a required permission. Reconnect and grant the requested access.",
        ));
    }
    Ok(())
}
