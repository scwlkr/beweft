use super::*;
pub fn validate(provider: &str, p: &Publication) -> Result<()> {
    if provider != "x" && p.options.get("x_thread").is_some() {
        return Err(invalid("X thread options are only supported on X channels"));
    }
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
    if moving > 0 && p.media.len() > 1 && !matches!(provider, "instagram" | "threads") {
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
            x::x_thread_replies(p)?;
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
            if p.options.get("format").is_some_and(|v| v != "reel") {
                return Err(invalid(
                    "Facebook format must be reel when publishing video",
                ));
            }
            if p.options["format"] == "reel" {
                if p.media.len() != 1
                    || p.media[0].mime != "video/mp4"
                    || !p.media[0].alt_text.trim().is_empty()
                {
                    return Err(invalid(
                        "Facebook Reels require one MP4; video descriptions are not supported by this adapter",
                    ));
                }
            } else if p.media.len() > 1 || p.media.iter().any(|m| !image(m)) {
                return Err(invalid(
                    "Facebook supports text, one image, or one video with format=reel. Standard video uses manual handoff.",
                ));
            }
        }
        "instagram" => {
            if p.options.get("format").is_some() {
                return Err(invalid(
                    "Instagram Stories are not implemented; use manual handoff. Carousels are selected by the attachment count.",
                ));
            }
            if p.media.is_empty()
                || p.media.len() > 10
                || p.media
                    .iter()
                    .any(|m| !matches!(m.mime.as_str(), "image/jpeg" | "video/mp4"))
            {
                return Err(invalid(
                    "Instagram supports one JPEG or MP4 Reel, or a carousel of 2–10 JPEG/MP4 items",
                ));
            }
            if p.media
                .iter()
                .any(|m| video(m) && !m.alt_text.trim().is_empty())
            {
                return Err(invalid(
                    "Instagram video descriptions are not supported by this adapter",
                ));
            }
            post_limit(p, 2200)?;
        }
        "threads" => {
            if p.options.get("format").is_some() {
                return Err(invalid(
                    "Threads carousels are selected by the attachment count; explicit format options are not supported.",
                ));
            }
            post_limit(p, 500)?;
            if p.options.get("reply_control").is_some_and(|v| {
                !matches!(
                    v.as_str(),
                    Some("everyone" | "accounts_you_follow" | "mentioned_only")
                )
            }) {
                return Err(invalid("Unsupported Threads reply control"));
            }
            if p.media.len() > 10
                || p.media
                    .iter()
                    .any(|m| !matches!(m.mime.as_str(), "image/jpeg" | "image/png" | "video/mp4"))
            {
                return Err(invalid(
                    "Threads supports text, one JPEG/PNG/MP4, or a carousel of 2–10 JPEG/PNG/MP4 items",
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
            if p.media.len() > 4 || p.media.iter().any(|m| !image(m) && m.mime != "video/mp4") {
                return Err(invalid(
                    "Bluesky supports up to four images or one MP4 video",
                ));
            }
        }
        "linkedin" => {
            post_limit(p, 3000)?;
            if p.media.len() > 20
                || p.media
                    .iter()
                    .any(|m| !matches!(m.mime.as_str(), "image/jpeg" | "image/png" | "video/mp4"))
                || p.options.get("alt_text").is_some_and(|v| !v.is_string())
                || p.media
                    .iter()
                    .any(|m| x_alt_text(p, m).chars().count() > 4086)
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
