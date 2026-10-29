use crate::*;
fn required_scopes(provider: &str, action: &str) -> Vec<&'static str> {
    match (provider, action) {
        ("x", "publish") => vec!["tweet.write"],
        ("x", "media") => vec!["tweet.write", "media.write"],
        ("x", "metrics") => vec!["tweet.read"],
        ("youtube", "publish" | "media") => vec!["https://www.googleapis.com/auth/youtube.upload"],
        ("youtube", "reply" | "inbox") => vec!["https://www.googleapis.com/auth/youtube.force-ssl"],
        ("youtube", "metrics") => vec!["https://www.googleapis.com/auth/youtube.readonly"],
        ("facebook", "publish" | "media") => vec!["pages_manage_posts"],
        ("facebook", "reply") => vec!["pages_manage_engagement"],
        ("facebook", "inbox") => vec!["pages_read_engagement"],
        ("facebook", "metrics") => vec!["pages_read_engagement"],
        ("instagram", "publish" | "media") => vec!["instagram_business_content_publish"],
        ("instagram", "inbox" | "reply") => vec!["instagram_business_manage_comments"],
        ("instagram", "metrics") => vec!["instagram_business_basic"],
        ("threads", "publish" | "media") => vec!["threads_content_publish"],
        ("threads", "inbox") => vec!["threads_read_replies"],
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
    for action in ["publish", "media", "metrics", "inbox", "reply"] {
        let required = required_scopes(&c.app.provider, action);
        let missing: Vec<_> = required
            .iter()
            .filter(|s| !c.tokens.scopes.iter().any(|g| g == **s))
            .collect();
        let status = if !c.tokens.scopes_known || required.is_empty() {
            "unknown"
        } else if missing.is_empty() {
            "granted"
        } else {
            "missing"
        };
        actions.insert(action.into(), json!({"status":status,"required_scopes":required,"missing_scopes":if c.tokens.scopes_known {json!(missing)} else {Value::Null}}));
    }
    json!({"source":if c.tokens.scopes_known {"provider_token_response"}else{"provider_did_not_report_scopes"},"reported_scopes":c.tokens.scopes,"actions":actions,"note":"Granted scopes do not prove format eligibility, provider app review, account access or successful publication."})
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
