use super::*;

pub(super) fn meta_id(value: &str) -> Result<&str> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(error(
            FailureKind::Uncertain,
            "Invalid Meta delivery checkpoint; inspect the provider before retrying",
        ));
    }
    Ok(value)
}

fn meta_children(cp: &Value, total: usize) -> Result<Vec<String>> {
    let items = cp["children"]
        .as_array()
        .ok_or_else(|| error(FailureKind::Uncertain, "Invalid Meta carousel checkpoint"))?;
    if items.len() > total {
        return Err(error(
            FailureKind::Uncertain,
            "Invalid Meta carousel checkpoint",
        ));
    }
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let id = item
            .as_str()
            .ok_or_else(|| error(FailureKind::Uncertain, "Invalid Meta carousel checkpoint"))?;
        meta_id(id)?;
        if result.iter().any(|previous| previous == id) {
            return Err(error(
                FailureKind::Uncertain,
                "Invalid Meta carousel checkpoint",
            ));
        }
        result.push(id.to_owned());
    }
    Ok(result)
}

fn meta_checkpoint(stage: &str, container: &str) -> Value {
    json!({"stage":stage,"container_id":container,"expires_after_secs":3600,"check_after_secs":60})
}

impl Adapter {
    pub(super) async fn publish_meta(
        &self,
        c: &Channel,
        p: &Publication,
        cp: &Value,
    ) -> Result<Step> {
        let token = Some(c.tokens.access_token.as_str());
        let account = id_path(&c.identity.id);
        let stage = checkpoint_stage(cp);
        let ig = c.app.provider == "instagram";
        let story = ig && p.options["format"] == "story";
        if story
            && self.fixture_origin.is_none()
            && !super::google_business::google_public_origin(public_media(&p.media[0])?)
        {
            return Err(invalid(
                "Instagram Stories require an actual public HTTPS media URL",
            ));
        }
        let base = if ig {
            "https://graph.instagram.com/v23.0"
        } else {
            "https://graph.threads.net/v1.0"
        };
        let edge = if ig { "media" } else { "threads" };
        let carousel = p.media.len() > 1;
        if stage.is_empty() && carousel {
            return Ok(Step::Checkpoint(
                json!({"stage":"meta_build","children":[],"expires_after_secs":3600}),
            ));
        }
        if stage == "meta_build" && !carousel {
            return Err(error(
                FailureKind::Uncertain,
                "Invalid Meta carousel checkpoint",
            ));
        }
        if stage.is_empty() || stage == "meta_build" {
            let children = if carousel {
                meta_children(cp, p.media.len())?
            } else {
                vec![]
            };
            let mut body = if carousel && children.len() == p.media.len() {
                let mut parent = json!({"media_type":"CAROUSEL","children":children.join(",")});
                parent[if ig { "caption" } else { "text" }] = json!(p.text);
                parent
            } else if carousel {
                let m = &p.media[children.len()];
                let mut item = json!({"is_carousel_item":true});
                if video(m) {
                    item["media_type"] = json!("VIDEO");
                    item["video_url"] = json!(public_media(m)?);
                } else {
                    if !ig {
                        item["media_type"] = json!("IMAGE");
                    }
                    item["image_url"] = json!(public_media(m)?);
                }
                if !m.alt_text.is_empty() && (!ig || image(m)) {
                    item["alt_text"] = json!(m.alt_text);
                }
                item
            } else {
                let mut single = if story {
                    json!({"media_type":"STORIES"})
                } else if ig {
                    json!({"caption":p.text})
                } else {
                    json!({"text":p.text,"media_type":"TEXT"})
                };
                if let Some(m) = p.media.first() {
                    if video(m) {
                        if !story {
                            single["media_type"] = json!(if ig { "REELS" } else { "VIDEO" });
                        }
                        single["video_url"] = json!(public_media(m)?);
                    } else {
                        if !ig {
                            single["media_type"] = json!("IMAGE");
                        }
                        single["image_url"] = json!(public_media(m)?);
                    }
                    if !story && !m.alt_text.is_empty() && (!ig || image(m)) {
                        single["alt_text"] = json!(m.alt_text);
                    }
                }
                single
            };
            // Reply control belongs to the parent, not to carousel items.
            if !ig
                && (!carousel || children.len() == p.media.len())
                && let Some(reply) = p.options["reply_control"].as_str()
            {
                body["reply_control"] = json!(reply);
            }
            let v = self
                .send(
                    self.request(Method::POST, &format!("{base}/{account}/{edge}"), token)?
                        .form(&body),
                    true,
                )
                .await?;
            let id = field(&v, "id")?;
            meta_id(&id)?;
            return Ok(Step::Checkpoint(
                if carousel && children.len() < p.media.len() {
                    json!({"stage":"meta_child","container_id":id,"children":children,"expires_after_secs":3600,"check_after_secs":60})
                } else {
                    let mut next = meta_checkpoint("meta_container", &id);
                    if carousel {
                        next["children"] = json!(children);
                    }
                    next
                },
            ));
        }
        if stage == "ready" {
            // A legacy ready checkpoint may already have issued its public write.
            return Err(error(
                FailureKind::Uncertain,
                "A previous Meta publication may have started. Inspect the provider before resolving its outcome.",
            ));
        }
        if !matches!(
            stage,
            "container" | "meta_child" | "meta_container" | "meta_ready" | "meta_public"
        ) {
            return Err(error(
                FailureKind::Uncertain,
                "Invalid Meta delivery checkpoint",
            ));
        }
        let container = field(cp, "container_id")?;
        meta_id(&container)?;
        if carousel && stage != "meta_child" {
            let children = meta_children(cp, p.media.len())?;
            if children.len() != p.media.len() || children.contains(&container) {
                return Err(error(
                    FailureKind::Uncertain,
                    "Invalid Meta carousel checkpoint",
                ));
            }
        }
        if stage == "container" || stage == "meta_child" || stage == "meta_container" {
            if stage == "meta_child" && !carousel {
                return Err(error(
                    FailureKind::Uncertain,
                    "Invalid Meta carousel checkpoint",
                ));
            }
            let children = if stage == "meta_child" {
                Some(meta_children(cp, p.media.len())?)
            } else {
                None
            };
            if children
                .as_ref()
                .is_some_and(|items| items.len() >= p.media.len() || items.contains(&container))
            {
                return Err(error(
                    FailureKind::Uncertain,
                    "Invalid Meta carousel checkpoint",
                ));
            }
            let v = self
                .send(
                    self.request(
                        Method::GET,
                        &format!("{base}/{}", id_path(&container)),
                        token,
                    )?
                    .query(&[("fields", if ig { "status_code" } else { "status" })]),
                    false,
                )
                .await?;
            if story && v["id"].as_str() != Some(container.as_str()) {
                return Err(error(
                    FailureKind::Uncertain,
                    "Instagram returned a different Story container identifier",
                ));
            }
            let status = v[if ig { "status_code" } else { "status" }]
                .as_str()
                .unwrap_or("");
            return match status {
                "FINISHED" => {
                    if let Some(mut children) = children {
                        children.push(container);
                        Ok(Step::Checkpoint(
                            json!({"stage":"meta_build","children":children,"expires_after_secs":3600}),
                        ))
                    } else {
                        let mut next = meta_checkpoint("meta_ready", &container);
                        if carousel {
                            next["children"] = cp["children"].clone();
                        }
                        Ok(Step::Checkpoint(next))
                    }
                }
                "IN_PROGRESS" => {
                    let polls = cp["polls"].as_u64().unwrap_or(0);
                    if polls >= 4 {
                        return Err(error(
                            FailureKind::ExpiredSession,
                            "Meta media processing did not finish within the supported polling window. Inspect the provider before retrying.",
                        ));
                    }
                    let mut wait = cp.clone();
                    wait["polls"] = json!(polls + 1);
                    wait["check_after_secs"] = json!(60);
                    Ok(Step::Wait(wait))
                }
                "PUBLISHED" => Err(error(
                    FailureKind::Uncertain,
                    "Container is already published; reconcile its publication ID",
                )),
                "ERROR" | "EXPIRED" => Err(invalid("Provider media processing failed or expired")),
                _ => Err(error(
                    FailureKind::Uncertain,
                    "Provider returned an unknown processing status",
                )),
            };
        }
        if stage == "meta_ready" {
            let mut next = meta_checkpoint("meta_public", &container);
            if carousel {
                next["children"] = cp["children"].clone();
            }
            return Ok(Step::Checkpoint(next));
        }
        let result = self
            .send(
                self.request(
                    Method::POST,
                    &format!(
                        "{base}/{account}/{}_publish",
                        if ig { "media" } else { "threads" }
                    ),
                    token,
                )?
                .form(&[("creation_id", &container)]),
                true,
            )
            .await;
        let v =
            if story {
                result.map_err(|_| error(FailureKind::Uncertain,
                "Instagram may have accepted the Story. Inspect its outcome before retrying."))?
            } else {
                result?
            };
        let id = field(&v, "id").map_err(|error| if story {
            crate::error(FailureKind::Uncertain,
                "Instagram did not return a usable Story ID. Inspect its outcome before retrying.")
        } else { error })?;
        meta_id(&id)?;
        Ok(published(
            id.clone(),
            None,
            json!({"id":id,"container_id":container}),
        ))
    }
}
