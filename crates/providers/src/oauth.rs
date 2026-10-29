use crate::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

pub fn scopes(provider: &str) -> Vec<String> {
    match provider {
        "x" => vec![
            "tweet.read",
            "tweet.write",
            "users.read",
            "offline.access",
            "media.write",
        ],
        "youtube" => vec![
            "https://www.googleapis.com/auth/youtube.upload",
            "https://www.googleapis.com/auth/youtube.readonly",
            "https://www.googleapis.com/auth/youtube.force-ssl",
            "https://www.googleapis.com/auth/yt-analytics.readonly",
        ],
        "google_business" => vec!["https://www.googleapis.com/auth/business.manage"],
        "facebook" => vec![
            "pages_show_list",
            "pages_read_engagement",
            "pages_manage_posts",
            "pages_manage_engagement",
            "read_insights",
        ],
        "instagram" => vec![
            "instagram_business_basic",
            "instagram_business_content_publish",
            "instagram_business_manage_comments",
            "instagram_business_manage_insights",
        ],
        "threads" => vec![
            "threads_basic",
            "threads_content_publish",
            "threads_manage_insights",
            "threads_read_replies",
            "threads_manage_replies",
        ],
        "mastodon" => vec!["read", "write"],
        "linkedin" => vec!["openid", "profile", "w_member_social"],
        "pinterest" => vec![
            "user_accounts:read",
            "boards:read",
            "pins:read",
            "pins:write",
        ],
        _ => vec![],
    }
    .into_iter()
    .map(str::to_owned)
    .collect()
}
fn auth_endpoint(app: &AppConfig) -> Result<String> {
    Ok(match app.provider.as_str() {
        "x" => "https://x.com/i/oauth2/authorize".into(),
        "youtube" | "google_business" => "https://accounts.google.com/o/oauth2/v2/auth".into(),
        "facebook" => "https://www.facebook.com/v23.0/dialog/oauth".into(),
        "instagram" => "https://www.instagram.com/oauth/authorize".into(),
        "threads" => "https://threads.net/oauth/authorize".into(),
        "mastodon" => format!("{}/oauth/authorize", instance_origin(app)?),
        "linkedin" => "https://www.linkedin.com/oauth/v2/authorization".into(),
        "pinterest" => "https://www.pinterest.com/oauth/".into(),
        _ => return Err(invalid("This provider does not support this OAuth flow")),
    })
}
fn token_endpoint(app: &AppConfig) -> Result<String> {
    Ok(match app.provider.as_str() {
        "x" => "https://api.x.com/2/oauth2/token".into(),
        "youtube" | "google_business" => "https://oauth2.googleapis.com/token".into(),
        "facebook" => "https://graph.facebook.com/v23.0/oauth/access_token".into(),
        "instagram" => "https://api.instagram.com/oauth/access_token".into(),
        "threads" => "https://graph.threads.net/oauth/access_token".into(),
        "mastodon" => format!("{}/oauth/token", instance_origin(app)?),
        "linkedin" => "https://www.linkedin.com/oauth/v2/accessToken".into(),
        "pinterest" => "https://api.pinterest.com/v5/oauth/token".into(),
        _ => return Err(invalid("This provider does not support this OAuth flow")),
    })
}
pub fn authorization_url(
    app: &AppConfig,
    redirect_uri: &str,
    state: &str,
    verifier: &str,
) -> Result<String> {
    let mut u = Url::parse(&auth_endpoint(app)?).map_err(|_| invalid("Invalid OAuth endpoint"))?;
    let requested = scopes(&app.provider).join(
        if matches!(
            app.provider.as_str(),
            "facebook" | "instagram" | "threads" | "pinterest"
        ) {
            ","
        } else {
            " "
        },
    );
    {
        let mut q = u.query_pairs_mut();
        q.append_pair("client_id", &app.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("state", state)
            .append_pair("scope", &requested);
        if matches!(
            app.provider.as_str(),
            "x" | "youtube" | "google_business" | "mastodon"
        ) {
            q.append_pair(
                "code_challenge",
                &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
            )
            .append_pair("code_challenge_method", "S256");
        }
        if matches!(app.provider.as_str(), "youtube" | "google_business") {
            q.append_pair("access_type", "offline")
                .append_pair("prompt", "consent");
        }
        if app.provider == "instagram" {
            q.append_pair("enable_fb_login", "0")
                .append_pair("force_authentication", "1");
        }
    }
    Ok(u.into())
}
fn parse_tokens(v: &Value, original: Option<&Tokens>) -> Result<Tokens> {
    let v = if v["data"].is_array() {
        &v["data"][0]
    } else {
        v
    };
    Ok(Tokens {
        scopes_known: v["scope"].is_string() || original.is_some_and(|t| t.scopes_known),
        access_token: field(v, "access_token")?,
        refresh_token: v["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| original.and_then(|t| t.refresh_token.clone())),
        expires_at: v["expires_in"]
            .as_i64()
            .map(|n| chrono::Utc::now().timestamp() + n),
        scopes: v["scope"]
            .as_str()
            .map(|s| {
                s.split([' ', ','])
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(|| original.map(|t| t.scopes.clone()).unwrap_or_default()),
    })
}

mod google_business;
mod identity;
mod tokens;
