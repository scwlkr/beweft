use super::*;
pub fn validate(provider: &str, p: &Publication) -> Result<()> {
    if p.text.trim().is_empty() && p.media.is_empty() {
        return Err(invalid("Post needs text or media"));
    }
    if p.media.iter().any(|m| m.source.is_empty()) {
        return Err(invalid("Media is empty"));
    }
    if p.media.iter().any(|m| {
        !matches!(
            m.mime.as_str(),
            "image/jpeg"
                | "image/png"
                | "image/webp"
                | "image/gif"
                | "video/mp4"
                | "video/quicktime"
                | "video/webm"
        )
    }) {
        return Err(invalid("Unsupported media type"));
    }
    let moving = p
        .media
        .iter()
        .filter(|m| video(m) || m.mime == "image/gif")
        .count();
    if moving > 0 && p.media.len() > 1 {
        return Err(invalid(
            "Animated images and video cannot be combined with other attachments",
        ));
    }
    if matches!(
        provider,
        "x" | "facebook" | "instagram" | "threads" | "pinterest" | "bluesky"
    ) && p.media.iter().any(|m| m.mime == "image/gif")
    {
        return Err(invalid(
            "Animated GIF is not supported by this provider adapter",
        ));
    }
    match provider {
        "x" => {
            post_limit(p, 280)?;
            if p.media.len() > 4
                || p.media.iter().any(|m| !image(m) && m.mime != "video/mp4")
                || p.options
                    .get("alt_text")
                    .is_some_and(|value| !value.is_string())
                || p.media
                    .iter()
                    .any(|m| x_alt_text(p, m).chars().count() > 1000)
            {
                return Err(invalid(
                    "X supports up to four images or one MP4 video; image descriptions must be at most 1,000 characters",
                ));
            }
            // X's documented alt-text support is for image media; do not
            // silently drop a video description where support is unverified.
            if p.media
                .iter()
                .any(|m| video(m) && !x_alt_text(p, m).trim().is_empty())
            {
                return Err(invalid(
                    "X video descriptions are not supported by this adapter",
                ));
            }
        }
        "youtube" => {
            if p.media.len() != 1
                || !video(&p.media[0])
                || p.title.trim().is_empty()
                || p.title.chars().count() > 100
            {
                return Err(invalid(
                    "YouTube needs one video and a title of 1–100 characters",
                ));
            }
            if !matches!(
                p.options["privacy"].as_str(),
                Some("private" | "unlisted" | "public")
            ) {
                return Err(invalid(
                    "Choose YouTube privacy explicitly: private, unlisted or public",
                ));
            }
            if !p.options["made_for_kids"].is_boolean() {
                return Err(invalid(
                    "Choose the YouTube made-for-kids audience setting explicitly",
                ));
            }
            if p.options
                .get("contains_synthetic_media")
                .is_some_and(|v| !v.is_boolean())
                || p.options.get("category_id").is_some_and(|v| !v.is_string())
            {
                return Err(invalid("Invalid YouTube publishing options"));
            }
            post_limit(p, 5000)?;
        }
        "facebook" => {
            if p.media.len() > 1 || p.media.iter().any(|m| !image(m)) {
                return Err(invalid(
                    "Facebook adapter supports text or one image. Use manual handoff for other formats.",
                ));
            }
        }
        "instagram" => {
            if p.media.len() != 1 || (!image(&p.media[0]) && !video(&p.media[0])) {
                return Err(invalid(
                    "Instagram adapter needs one image or Reel. Other formats use manual handoff.",
                ));
            }
            if image(&p.media[0]) && p.media[0].mime != "image/jpeg" {
                return Err(invalid("Instagram single-image publishing requires JPEG"));
            }
            post_limit(p, 2200)?;
        }
        "threads" => {
            post_limit(p, 500)?;
            if p.options.get("reply_control").is_some_and(|v| {
                !matches!(
                    v.as_str(),
                    Some("everyone" | "accounts_you_follow" | "mentioned_only")
                )
            }) {
                return Err(invalid("Unsupported Threads reply control"));
            }
            if p.media.len() > 1 {
                return Err(invalid(
                    "Threads adapter supports one media attachment; carousel uses manual handoff",
                ));
            }
        }
        "mastodon" => {
            if p.options.get("visibility").is_some_and(|v| {
                !matches!(
                    v.as_str(),
                    Some("public" | "unlisted" | "private" | "direct")
                )
            }) {
                return Err(invalid("Invalid Mastodon visibility"));
            }
            if p.options
                .get("content_warning")
                .is_some_and(|v| !v.is_string())
                || p.options.get("sensitive").is_some_and(|v| !v.is_boolean())
            {
                return Err(invalid("Invalid Mastodon publishing options"));
            }
            if p.media.len() > 4 {
                return Err(invalid("Mastodon adapter supports up to four attachments"));
            }
        }
        "bluesky" => {
            post_limit(p, 300)?;
            if p.media.len() > 4 || p.media.iter().any(|m| !image(m)) {
                return Err(invalid(
                    "Bluesky adapter supports up to four images. Video uses manual handoff.",
                ));
            }
        }
        "linkedin" => {
            post_limit(p, 3000)?;
            if p.media.len() > 20
                || p.media
                    .iter()
                    .any(|m| !matches!(m.mime.as_str(), "image/jpeg" | "image/png" | "video/mp4"))
                || p.media.iter().any(|m| m.alt_text.chars().count() > 4086)
            {
                return Err(invalid(
                    "LinkedIn supports up to 20 JPEG/PNG images or one MP4 video; descriptions must be at most 4,086 characters",
                ));
            }
        }
        "pinterest" => {
            if p.media.len() != 1
                || !image(&p.media[0])
                || p.options["board_id"].as_str().unwrap_or("").is_empty()
            {
                return Err(invalid("Pinterest needs one image and a board ID"));
            }
            if p.options
                .get("link")
                .is_some_and(|v| !v.is_string() && !v.is_null())
            {
                return Err(invalid("Invalid Pinterest link"));
            }
        }
        _ => return Err(invalid("No direct publishing adapter for this provider")),
    }
    Ok(())
}
