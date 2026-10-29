use super::*;

const VERSION: &str = "202610";
const PART: u64 = 4 * 1024 * 1024;

fn uncertain(message: &str) -> ProviderError {
    error(FailureKind::Uncertain, message)
}

fn urn(value: &Value, key: &str, kind: &str) -> Result<String> {
    let id = field(value, key)?;
    let suffix = id.strip_prefix(&format!("urn:li:{kind}:"));
    if suffix.is_none_or(|s| {
        s.is_empty()
            || s.len() > 200
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    }) {
        return Err(uncertain("LinkedIn returned an invalid identifier"));
    }
    Ok(id)
}

// Signed upload URLs are secrets. Send image authorization only to LinkedIn's
// documented upload origin, never to a provider-supplied arbitrary host.
fn upload_url(value: &Value, key: &str) -> Result<String> {
    let raw = field(value, key)?;
    let url = Url::parse(&raw).map_err(|_| uncertain("LinkedIn returned an invalid upload URL"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("www.linkedin.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with("/dms-uploads/")
    {
        return Err(uncertain("LinkedIn returned an untrusted upload URL"));
    }
    Ok(raw)
}

fn expires(value: &Value, key: &str) -> Result<i64> {
    value[key]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(|| uncertain("LinkedIn omitted the upload URL expiry"))
}

fn description<'a>(p: &'a Publication, media: &'a Media) -> &'a str {
    if media.alt_text.trim().is_empty() {
        p.options["alt_text"].as_str().unwrap_or("")
    } else {
        &media.alt_text
    }
}

fn uploaded_ids(cp: &Value, p: &Publication) -> Result<Vec<Value>> {
    let ids = cp["media_ids"]
        .as_array()
        .cloned()
        .ok_or_else(|| uncertain("Invalid LinkedIn media checkpoint"))?;
    if ids.len() > p.media.len() {
        return Err(uncertain("Invalid LinkedIn media progress"));
    }
    for id in &ids {
        urn(
            &json!({"id":id}),
            "id",
            if p.media.first().is_some_and(video) {
                "video"
            } else {
                "image"
            },
        )?;
    }
    Ok(ids)
}

fn headers(b: RequestBuilder) -> RequestBuilder {
    b.header("Linkedin-Version", VERSION)
        .header("X-Restli-Protocol-Version", "2.0.0")
}

impl Adapter {
    pub(super) async fn publish_linkedin(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        let token = Some(c.tokens.access_token.as_str());
        let stage = checkpoint_stage(cp);
        let author = format!("urn:li:person:{}", c.identity.id);
        match stage {
            "" => {
                return Ok(Step::Checkpoint(
                    json!({"stage":if p.media.is_empty() {"linkedin_post_started"} else if video(&p.media[0]) {"linkedin_video_init"} else {"linkedin_image_init"},"media_ids":[]}),
                ));
            }
            "linkedin_image_init" => {
                let ids = uploaded_ids(cp, p)?;
                if ids.len() == p.media.len() {
                    let mut next = cp.clone();
                    next["stage"] = json!("linkedin_post_started");
                    return Ok(Step::Checkpoint(next));
                }
                let value = self
                    .send(
                        headers(self.request(
                            Method::POST,
                            "https://api.linkedin.com/rest/images?action=initializeUpload",
                            token,
                        )?)
                        .json(&json!({"initializeUploadRequest":{"owner":author}})),
                        true,
                    )
                    .await?;
                let value = &value["value"];
                let mut next = cp.clone();
                next["stage"] = json!("linkedin_image_upload");
                next["upload_id"] = json!(urn(value, "image", "image")?);
                next["upload_url"] = json!(upload_url(value, "uploadUrl")?);
                next["upload_expires_ms"] = json!(expires(value, "uploadUrlExpiresAt")?);
                return Ok(Step::Checkpoint(next));
            }
            "linkedin_image_upload" => {
                let ids = uploaded_ids(cp, p)?;
                let media = p
                    .media
                    .get(ids.len())
                    .ok_or_else(|| uncertain("Invalid LinkedIn image progress"))?;
                urn(cp, "upload_id", "image")?;
                let url = upload_url(cp, "upload_url")?;
                expires(cp, "upload_expires_ms")?;
                let response = self
                    .response(
                        self.request(Method::PUT, &url, token)?
                            .header("Content-Type", &media.mime)
                            .header("Content-Length", media.source.len())
                            .body(media.source.body(0, media.source.len()).await?),
                        true,
                    )
                    .await?;
                if !matches!(response.status().as_u16(), 200 | 201 | 204) {
                    return Err(uncertain(
                        "LinkedIn returned an unexpected image upload receipt",
                    ));
                }
                let mut next = cp.clone();
                next["stage"] = json!("linkedin_image_processing");
                return Ok(Step::Checkpoint(next));
            }
            "linkedin_image_processing" => {
                let id = urn(cp, "upload_id", "image")?;
                // LinkedIn documents that w_member_social permits this legacy read,
                // while the same read through its versioned gateway is forbidden.
                let value = self
                    .send(
                        self.request(
                            Method::GET,
                            &format!("https://api.linkedin.com/rest/images/{}", id_path(&id)),
                            token,
                        )?
                        .header("X-Restli-Protocol-Version", "2.0.0"),
                        false,
                    )
                    .await?;
                if urn(&value, "id", "image")? != id || value["owner"] != author {
                    return Err(uncertain("LinkedIn image status did not match the upload"));
                }
                match value["status"].as_str() {
                    Some("AVAILABLE") => {
                        let mut ids = uploaded_ids(cp, p)?;
                        ids.push(json!(id));
                        let mut next = cp.clone();
                        next["media_ids"] = json!(ids);
                        next["stage"] = json!("linkedin_image_init");
                        return Ok(Step::Checkpoint(next));
                    }
                    Some("PROCESSING" | "WAITING_UPLOAD") => return Ok(Step::Wait(cp.clone())),
                    Some("PROCESSING_FAILED") => {
                        return Err(invalid(
                            "LinkedIn could not process the image; no post was created",
                        ));
                    }
                    _ => {
                        return Err(uncertain(
                            "LinkedIn returned an unknown image processing state",
                        ));
                    }
                }
            }
            "linkedin_video_init" => {
                if p.media.len() != 1 || !video(&p.media[0]) {
                    return Err(uncertain("Invalid LinkedIn video checkpoint"));
                }
                let size = p.media[0].source.len();
                let value=self.send(headers(self.request(Method::POST,"https://api.linkedin.com/rest/videos?action=initializeUpload",token)?)
                    .json(&json!({"initializeUploadRequest":{"owner":author,"fileSizeBytes":size,"uploadCaptions":false,"uploadThumbnail":false}})),true).await?;
                let value = &value["value"];
                let id = urn(value, "video", "video")?;
                let instructions = value["uploadInstructions"]
                    .as_array()
                    .filter(|v| !v.is_empty() && v.len() <= 64)
                    .ok_or_else(|| uncertain("Invalid LinkedIn video upload instructions"))?;
                let mut first = 0;
                for part in instructions {
                    upload_url(part, "uploadUrl")?;
                    let start = part["firstByte"]
                        .as_u64()
                        .ok_or_else(|| uncertain("Invalid LinkedIn video range"))?;
                    let end = part["lastByte"]
                        .as_u64()
                        .ok_or_else(|| uncertain("Invalid LinkedIn video range"))?;
                    if start != first || end < start || end - start >= PART || start >= size {
                        return Err(uncertain("Invalid LinkedIn video range"));
                    }
                    first = end.saturating_add(1).min(size);
                }
                if first != size || !value["uploadToken"].is_string() {
                    return Err(uncertain("Incomplete LinkedIn video upload instructions"));
                }
                let mut next = cp.clone();
                next["stage"] = json!("linkedin_video_upload");
                next["upload_id"] = json!(id);
                next["upload_expires_ms"] = json!(expires(value, "uploadUrlsExpireAt")?);
                next["instructions"] = json!(instructions);
                next["upload_token"] = value["uploadToken"].clone();
                next["part_ids"] = json!([]);
                next["total_bytes"] = json!(size);
                return Ok(Step::Checkpoint(next));
            }
            "linkedin_video_upload" => {
                urn(cp, "upload_id", "video")?;
                expires(cp, "upload_expires_ms")?;
                let media = p
                    .media
                    .first()
                    .filter(|m| video(m) && cp["total_bytes"].as_u64() == Some(m.source.len()))
                    .ok_or_else(|| uncertain("Invalid LinkedIn video snapshot"))?;
                let parts = cp["instructions"]
                    .as_array()
                    .ok_or_else(|| uncertain("Invalid LinkedIn video instructions"))?;
                let mut receipts = cp["part_ids"]
                    .as_array()
                    .cloned()
                    .ok_or_else(|| uncertain("Invalid LinkedIn video receipts"))?;
                let part = parts
                    .get(receipts.len())
                    .ok_or_else(|| uncertain("Invalid LinkedIn video progress"))?;
                let start = part["firstByte"]
                    .as_u64()
                    .ok_or_else(|| uncertain("Invalid LinkedIn video range"))?;
                let end = part["lastByte"]
                    .as_u64()
                    .filter(|end| *end >= start && *end - start < PART)
                    .ok_or_else(|| uncertain("Invalid LinkedIn video range"))?
                    .min(media.source.len() - 1);
                let length = end
                    .checked_sub(start)
                    .and_then(|n| n.checked_add(1))
                    .ok_or_else(|| uncertain("Invalid LinkedIn video range"))?;
                let response = self
                    .response(
                        self.request(Method::PUT, &upload_url(part, "uploadUrl")?, None)?
                            .header("Content-Type", "application/octet-stream")
                            .header("Content-Length", length)
                            .body(media.source.body(start, length).await?),
                        true,
                    )
                    .await?;
                if response.status().as_u16() != 200 {
                    return Err(uncertain(
                        "LinkedIn returned an unexpected video upload receipt",
                    ));
                }
                let etag = response
                    .headers()
                    .get("etag")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.trim_matches('"'))
                    .filter(|v| !v.is_empty() && v.len() <= 4096)
                    .ok_or_else(|| uncertain("LinkedIn omitted the video part receipt"))?;
                receipts.push(json!(etag));
                let mut next = cp.clone();
                next["part_ids"] = json!(receipts);
                if next["part_ids"].as_array().unwrap().len() == parts.len() {
                    next["stage"] = json!("linkedin_video_finalize");
                }
                return Ok(Step::Checkpoint(next));
            }
            "linkedin_video_finalize" => {
                let id = urn(cp, "upload_id", "video")?;
                let parts = cp["instructions"]
                    .as_array()
                    .ok_or_else(|| uncertain("Invalid LinkedIn video instructions"))?;
                let receipts = cp["part_ids"]
                    .as_array()
                    .filter(|v| {
                        v.len() == parts.len()
                            && !v.is_empty()
                            && v.iter().all(|s| s.as_str().is_some_and(|s| !s.is_empty()))
                    })
                    .ok_or_else(|| uncertain("Incomplete LinkedIn video receipts"))?;
                let upload_token = cp["upload_token"]
                    .as_str()
                    .ok_or_else(|| uncertain("Invalid LinkedIn video upload token"))?;
                let response=self.response(headers(self.request(Method::POST,"https://api.linkedin.com/rest/videos?action=finalizeUpload",token)?)
                    .json(&json!({"finalizeUploadRequest":{"video":id,"uploadToken":upload_token,"uploadedPartIds":receipts}})),true).await?;
                if response.status().as_u16() != 200 {
                    return Err(uncertain(
                        "LinkedIn returned an unexpected video finalization receipt",
                    ));
                }
                let mut next = cp.clone();
                next["stage"] = json!("linkedin_video_processing");
                return Ok(Step::Checkpoint(next));
            }
            "linkedin_video_processing" => {
                let id = urn(cp, "upload_id", "video")?;
                let value = self
                    .send(
                        headers(self.request(
                            Method::GET,
                            &format!("https://api.linkedin.com/rest/videos/{}", id_path(&id)),
                            token,
                        )?),
                        false,
                    )
                    .await?;
                if urn(&value, "id", "video")? != id || value["owner"] != author {
                    return Err(uncertain("LinkedIn video status did not match the upload"));
                }
                match value["status"].as_str() {
                    Some("AVAILABLE") => {
                        let mut next = cp.clone();
                        next["stage"] = json!("linkedin_post_started");
                        next["media_ids"] = json!([id]);
                        return Ok(Step::Checkpoint(next));
                    }
                    Some("PROCESSING" | "WAITING_UPLOAD") => return Ok(Step::Wait(cp.clone())),
                    Some("PROCESSING_FAILED") => {
                        return Err(invalid(
                            "LinkedIn could not process the video; no post was created",
                        ));
                    }
                    _ => {
                        return Err(uncertain(
                            "LinkedIn returned an unknown video processing state",
                        ));
                    }
                }
            }
            "linkedin_post_started" => {
                let ids = uploaded_ids(cp, p)?;
                if ids.len() != p.media.len() {
                    return Err(uncertain("Incomplete LinkedIn post media checkpoint"));
                }
                let mut body = json!({"author":author,"commentary":p.text,"visibility":"PUBLIC","distribution":{"feedDistribution":"MAIN_FEED","targetEntities":[],"thirdPartyDistributionChannels":[]},"lifecycleState":"PUBLISHED","isReshareDisabledByAuthor":false});
                if ids.len() == 1 {
                    let mut media = json!({"id":ids[0]});
                    if !description(p, &p.media[0]).is_empty() {
                        media["altText"] = json!(description(p, &p.media[0]));
                    }
                    if video(&p.media[0]) && !p.title.is_empty() {
                        media["title"] = json!(p.title);
                    }
                    body["content"] = json!({"media":media});
                } else if ids.len() > 1 {
                    let images: Vec<Value> = ids
                        .iter()
                        .zip(&p.media)
                        .map(|(id, m)| {
                            let mut v = json!({"id":id});
                            if !description(p, m).is_empty() {
                                v["altText"] = json!(description(p, m));
                            }
                            v
                        })
                        .collect();
                    body["content"] = json!({"multiImage":{"images":images}});
                }
                let response = self
                    .response(
                        headers(self.request(
                            Method::POST,
                            "https://api.linkedin.com/rest/posts",
                            token,
                        )?)
                        .json(&body),
                        true,
                    )
                    .await?;
                if response.status().as_u16() != 201 {
                    return Err(uncertain("LinkedIn did not confirm post creation"));
                }
                let id = response
                    .headers()
                    .get("x-restli-id")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| uncertain("LinkedIn omitted the post receipt"))?;
                let v = json!({"id":id});
                let id = urn(
                    &v,
                    "id",
                    if id.starts_with("urn:li:share:") {
                        "share"
                    } else {
                        "ugcPost"
                    },
                )?;
                return Ok(published(
                    id.clone(),
                    Some(format!("https://www.linkedin.com/feed/update/{id}")),
                    json!({"id":id}),
                ));
            }
            _ => {}
        }
        Err(uncertain("Invalid LinkedIn delivery checkpoint"))
    }
}
