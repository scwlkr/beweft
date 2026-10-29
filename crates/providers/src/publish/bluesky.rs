use super::*;
use sha2::{Digest, Sha256};

const VIDEO_SERVICE: &str = "https://video.bsky.app";

fn uncertain(message: &str) -> ProviderError {
    error(FailureKind::Uncertain, message)
}

pub(super) fn valid_blob(blob: &Value, mime: &str) -> bool {
    blob["$type"] == "blob"
        && blob["mimeType"] == mime
        && blob["ref"]["$link"]
            .as_str()
            .is_some_and(|id| !id.is_empty() && id.len() <= 256)
        && blob["size"]
            .as_u64()
            .is_some_and(|size| size > 0 && size <= 300_000_000)
}

pub(super) fn bluesky_record(p: &Publication, cp: &Value) -> Result<Value> {
    let blobs = cp["blobs"]
        .as_array()
        .filter(|blobs| blobs.len() == p.media.len())
        .ok_or_else(|| uncertain("Bluesky media receipts do not match the publication snapshot"))?;
    let created = field(cp, "created_at")?;
    let mut record = json!({"$type":"app.bsky.feed.post","text":p.text,"createdAt":created});
    if let Some(media) = p.media.first() {
        if video(media) {
            if !valid_blob(&blobs[0], "video/mp4") {
                return Err(uncertain("Bluesky video receipt is incomplete"));
            }
            record["embed"] =
                json!({"$type":"app.bsky.embed.video","video":blobs[0],"alt":media.alt_text});
        } else {
            if blobs
                .iter()
                .zip(&p.media)
                .any(|(blob, media)| !valid_blob(blob, &media.mime))
            {
                return Err(uncertain("Bluesky image receipt is incomplete"));
            }
            record["embed"] = json!({"$type":"app.bsky.embed.images","images":blobs.iter().enumerate().map(|(i,b)|json!({"image":b,"alt":p.media[i].alt_text})).collect::<Vec<_>>()});
        }
    }
    Ok(record)
}

pub(super) fn bluesky_receipt(
    c: &Channel,
    cp: &Value,
    value: &Value,
    reconciled: bool,
) -> Result<Step> {
    let rkey = field(cp, "rkey")?;
    let expected = format!("at://{}/app.bsky.feed.post/{rkey}", c.identity.id);
    let uri = field(value, "uri")?;
    let cid = field(value, "cid")?;
    if uri != expected {
        return Err(uncertain("Bluesky returned a different record URI"));
    }
    Ok(published(
        uri.clone(),
        Some(format!(
            "https://bsky.app/profile/{}/post/{rkey}",
            c.identity.id
        )),
        json!({"uri":uri,"cid":cid,"reconciled":reconciled}),
    ))
}

fn job_step(value: &Value, c: &Channel, cp: &Value, expected_job: Option<&str>) -> Result<Step> {
    // The upload endpoint has historically returned the job either directly or
    // under jobStatus; the status endpoint's lexicon wraps it.
    let job = value.get("jobStatus").unwrap_or(value);
    let job_id = field(job, "jobId")?;
    let state = field(job, "state")?;
    if job_id.len() > 256
        || job["did"].as_str() != Some(c.identity.id.as_str())
        || expected_job.is_some_and(|id| id != job_id)
    {
        return Err(uncertain("Bluesky returned a different video job"));
    }
    let blob = &job["blob"];
    if !blob.is_null() {
        if !valid_blob(blob, "video/mp4")
            || (state == "JOB_STATE_FAILED" && job["error"] != "already_exists")
        {
            return Err(uncertain("Bluesky returned an invalid video receipt"));
        }
        let mut next = cp.clone();
        next["stage"] = json!("bsky_prepared");
        next["blobs"] = json!([blob]);
        next["job_id"] = json!(job_id);
        return Ok(Step::Checkpoint(next));
    }
    if state == "JOB_STATE_FAILED" {
        return Err(invalid(
            "Bluesky could not process the video; no post was created. Check video eligibility and format.",
        ));
    }
    // The lexicon treats unrecognized states as still processing. A completed
    // job without its blob is also polled within the local processing bound.
    let mut next = cp.clone();
    next["stage"] = json!("bsky_video_processing");
    next["job_id"] = json!(job_id);
    next["check_after_secs"] = json!(2);
    next["expires_after_secs"] = json!(1800);
    Ok(Step::Wait(next))
}

impl Adapter {
    pub(super) async fn publish_bluesky(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        let token = Some(c.tokens.access_token.as_str());
        let stage = checkpoint_stage(cp);
        let origin = instance_origin(&c.app)?;
        if stage.is_empty() {
            let micros = chrono::Utc::now().timestamp_micros() as u64;
            let hash = Sha256::digest(p.key.as_bytes());
            let mut n = (micros << 10) | (u16::from_be_bytes([hash[0], hash[1]]) as u64 & 1023);
            let alphabet = b"234567abcdefghijklmnopqrstuvwxyz";
            let mut chars = [b'2'; 13];
            for i in (0..13).rev() {
                chars[i] = alphabet[(n & 31) as usize];
                n >>= 5;
            }
            return Ok(Step::Checkpoint(
                json!({"stage":"bsky_prepared","rkey":String::from_utf8(chars.to_vec()).unwrap(),"created_at":chrono::Utc::now().to_rfc3339(),"blobs":[]}),
            ));
        }
        match stage {
            // Older checkpoints lack a persisted public-write boundary.
            "prepared" => Err(uncertain(
                "An older Bluesky attempt may have started. Reconcile its record before recovery.",
            )),
            "bsky_prepared" => {
                let blobs = cp["blobs"]
                    .as_array()
                    .ok_or_else(|| uncertain("Invalid Bluesky media checkpoint"))?;
                if blobs.len() > p.media.len() {
                    return Err(uncertain("Invalid Bluesky media checkpoint"));
                }
                let mut next = cp.clone();
                if blobs.len() < p.media.len() {
                    next["stage"] = json!(if video(&p.media[blobs.len()]) {
                        "bsky_video_preflight"
                    } else {
                        "bsky_image_upload_started"
                    });
                } else {
                    bluesky_record(p, cp)?;
                    next["stage"] = json!("bsky_post_started");
                }
                Ok(Step::Checkpoint(next))
            }
            "bsky_image_upload_started" => {
                let mut blobs = cp["blobs"]
                    .as_array()
                    .cloned()
                    .ok_or_else(|| uncertain("Invalid Bluesky image checkpoint"))?;
                let media = p
                    .media
                    .get(blobs.len())
                    .filter(|m| image(m))
                    .ok_or_else(|| uncertain("Invalid Bluesky image checkpoint"))?;
                let value = self
                    .send(
                        self.request(
                            Method::POST,
                            &format!("{origin}/xrpc/com.atproto.repo.uploadBlob"),
                            token,
                        )?
                        .header("Content-Type", &media.mime)
                        .header("Content-Length", media.source.len())
                        .body(media.source.body(0, media.source.len()).await?),
                        true,
                    )
                    .await?;
                if !valid_blob(&value["blob"], &media.mime) {
                    return Err(uncertain("Bluesky upload returned no usable image blob"));
                }
                blobs.push(value["blob"].clone());
                let mut next = cp.clone();
                next["stage"] = json!("bsky_prepared");
                next["blobs"] = json!(blobs);
                Ok(Step::Checkpoint(next))
            }
            "bsky_video_preflight" => {
                let session = self
                    .send(
                        self.request(
                            Method::GET,
                            &format!("{origin}/xrpc/com.atproto.server.getSession"),
                            token,
                        )?,
                        false,
                    )
                    .await?;
                if session["did"].as_str() != Some(c.identity.id.as_str())
                    || session["didDoc"]["id"].as_str() != Some(c.identity.id.as_str())
                    || session["active"] == false
                {
                    return Err(invalid("Bluesky session identity could not be verified"));
                }
                if session["emailConfirmed"] == false {
                    return Err(invalid(
                        "Verify the Bluesky account email before uploading video",
                    ));
                }
                let services = session["didDoc"]["service"]
                    .as_array()
                    .ok_or_else(|| invalid("Bluesky session did not identify its PDS"))?;
                let endpoints: Vec<_> = services
                    .iter()
                    .filter(|service| {
                        service["id"]
                            .as_str()
                            .is_some_and(|id| id.ends_with("#atproto_pds"))
                    })
                    .collect();
                if endpoints.len() != 1 {
                    return Err(invalid("Bluesky session did not identify a single PDS"));
                }
                let endpoint = endpoints[0]["serviceEndpoint"]
                    .as_str()
                    .filter(|endpoint| !endpoint.is_empty())
                    .ok_or_else(|| invalid("Bluesky session did not identify its PDS"))?;
                let mut pds = c.app.clone();
                pds.instance_url = Some(endpoint.to_owned());
                let pds_origin = instance_origin(&pds)
                    .map_err(|_| invalid("Bluesky returned an invalid PDS origin"))?;
                if pds_origin != origin {
                    return Err(invalid(
                        "This account uses a different PDS. Check the Bluesky provider setup before connecting at that approved origin.",
                    ));
                }
                let host = Url::parse(&pds_origin)
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_owned))
                    .ok_or_else(|| invalid("Bluesky returned an invalid PDS host"))?;
                let mut next = cp.clone();
                next["stage"] = json!("bsky_video_upload_started");
                next["pds_audience"] = json!(format!("did:web:{host}"));
                Ok(Step::Checkpoint(next))
            }
            "bsky_video_upload_started" => {
                let media = p
                    .media
                    .first()
                    .filter(|m| p.media.len() == 1 && m.mime == "video/mp4")
                    .ok_or_else(|| uncertain("Invalid Bluesky video checkpoint"))?;
                let audience = field(cp, "pds_audience")?;
                let expiry = (chrono::Utc::now().timestamp() + 1800).to_string();
                let auth = self
                    .send(
                        self.request(
                            Method::GET,
                            &format!("{origin}/xrpc/com.atproto.server.getServiceAuth"),
                            token,
                        )?
                        .query(&[
                            ("aud", audience.as_str()),
                            ("lxm", "com.atproto.repo.uploadBlob"),
                            ("exp", expiry.as_str()),
                        ]),
                        false,
                    )
                    .await?;
                let service_token = field(&auth, "token")?;
                let response = self
                    .raw_response(
                        self.request(
                            Method::POST,
                            &format!("{VIDEO_SERVICE}/xrpc/app.bsky.video.uploadVideo"),
                            Some(&service_token),
                        )?
                        .query(&[("did", c.identity.id.as_str()), ("name", "video.mp4")])
                        .header("Content-Type", "video/mp4")
                        .header("Content-Length", media.source.len())
                        .body(media.source.body(0, media.source.len()).await?),
                        true,
                    )
                    .await?;
                let status = response.status();
                if status.as_u16() == 429 {
                    // This response definitively rejected the private upload.
                    // Persist it before allowing a deliberate later retry.
                    let mut next = cp.clone();
                    next["stage"] = json!("bsky_video_upload_rejected");
                    return Ok(Step::Checkpoint(next));
                }
                if !status.is_success() && status.as_u16() != 409 {
                    return Err(match status.as_u16() {
                        401 => error(
                            FailureKind::Authentication,
                            "Bluesky authorization expired or was revoked. Reconnect this channel.",
                        ),
                        500..=599 => uncertain("Bluesky video upload outcome is unknown"),
                        _ => invalid(
                            "Bluesky rejected the video upload. Check account email verification, video eligibility, and quota.",
                        ),
                    });
                }
                let value = self.response_json(response, true).await?;
                if status.as_u16() == 409
                    && value["error"] != "already_exists"
                    && value["jobStatus"]["error"] != "already_exists"
                {
                    return Err(uncertain(
                        "Bluesky video upload returned an unresolved conflict",
                    ));
                }
                job_step(&value, c, cp, None)
            }
            "bsky_video_upload_rejected" => {
                Err(error(FailureKind::RateLimited, BLUESKY_VIDEO_RATE_LIMIT))
            }
            "bsky_video_processing" => {
                let job_id = field(cp, "job_id")?;
                let value = self
                    .send(
                        self.request(
                            Method::GET,
                            &format!("{VIDEO_SERVICE}/xrpc/app.bsky.video.getJobStatus"),
                            None,
                        )?
                        .query(&[("jobId", job_id.as_str())]),
                        false,
                    )
                    .await
                    .map_err(|error| {
                        if error.kind == FailureKind::Rejected {
                            uncertain("Bluesky video processing status could not be verified")
                        } else {
                            error
                        }
                    })?;
                job_step(&value, c, cp, Some(&job_id))
            }
            "bsky_post_started" => {
                let record = bluesky_record(p, cp)?;
                let value = self.send(self.request(Method::POST, &format!("{origin}/xrpc/com.atproto.repo.createRecord"), token)?
                    .json(&json!({"repo":c.identity.id,"collection":"app.bsky.feed.post","rkey":cp["rkey"],"record":record})), true).await.map_err(|error| {
                        if error.kind == FailureKind::Rejected {
                            uncertain("Bluesky did not confirm the post; reconcile its original record URI")
                        } else {
                            error
                        }
                    })?;
                bluesky_receipt(c, cp, &value, false)
            }
            _ => Err(uncertain("Invalid Bluesky delivery checkpoint")),
        }
    }
}
