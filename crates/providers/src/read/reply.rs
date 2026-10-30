use super::*;
impl Adapter {
    pub async fn reply(
        &self,
        c: &Channel,
        item: &InboxItem,
        text: &str,
        idempotency_key: &str,
    ) -> Result<Step> {
        if text.trim().is_empty() || text.len() > 10000 {
            return Err(invalid("Reply text is empty or too long"));
        }
        require_permission(c, "reply")?;
        let item_id = &item.id;
        let token = Some(c.tokens.access_token.as_str());
        let id = segment(item_id);
        let request = match c.app.provider.as_str() {
            "youtube" => self
                .request(
                    Method::POST,
                    "https://www.googleapis.com/youtube/v3/comments?part=snippet",
                    token,
                )?
                .json(&json!({"snippet":{"parentId":item_id,"textOriginal":text}})),
            "facebook" => self
                .request(
                    Method::POST,
                    &format!("https://graph.facebook.com/v23.0/{id}/comments"),
                    token,
                )?
                .json(&json!({"message":text})),
            "instagram" => self
                .request(
                    Method::POST,
                    &format!("https://graph.instagram.com/v23.0/{id}/replies"),
                    token,
                )?
                .json(&json!({"message":text})),
            "mastodon" => {
                let context = item.reply_context.as_ref().ok_or_else(|| {
                    invalid("Refresh this conversation to retrieve its audience before replying")
                })?;
                if !matches!(
                    context.visibility.as_str(),
                    "public" | "unlisted" | "private" | "direct"
                ) || context.recipients.is_empty()
                {
                    return Err(invalid(
                        "Conversation audience is incomplete; reply on Mastodon",
                    ));
                }
                // Prevent added mentions changing the recipient set. Prefix only the preserved participants.
                if text.contains('@')
                    || context.recipients.iter().any(|r| {
                        r.acct.is_empty()
                            || r.acct
                                .chars()
                                .any(|c| !(c.is_ascii_alphanumeric() || "_@.-".contains(c)))
                    })
                {
                    return Err(invalid(
                        "Replies cannot add mentions; original participants are included automatically",
                    ));
                }
                // A private source is visible to its author's followers, not ours. Narrow
                // the reply to its participants instead of disclosing it to our followers.
                let visibility = if matches!(context.visibility.as_str(), "private" | "direct") {
                    "direct"
                } else {
                    context.visibility.as_str()
                };
                let mentions = context
                    .recipients
                    .iter()
                    .map(|r| format!("@{}", r.acct))
                    .collect::<Vec<_>>()
                    .join(" ");
                self.request(Method::POST, &format!("{}/api/v1/statuses", instance_origin(&c.app)?), token)?
                    .header("Idempotency-Key", idempotency_key)
                    .json(&json!({"status":format!("{mentions} {text}"),"in_reply_to_id":item_id,"visibility":visibility,"allowed_mentions":context.recipients.iter().map(|r| &r.id).collect::<Vec<_>>()}))
            }
            "bluesky" => {
                if text.chars().count() > 300 {
                    return Err(invalid(
                        "Bluesky reply text exceeds the supported length",
                    ));
                }
                let origin = instance_origin(&c.app)?;
                let mut uri_parts = item_id
                    .strip_prefix("at://")
                    .unwrap_or(item_id)
                    .splitn(3, '/');
                let repo = uri_parts
                    .next()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| invalid("Bluesky reply target is invalid"))?
                    .to_owned();
                let collection = uri_parts.next().unwrap_or("");
                let rkey = uri_parts.next().unwrap_or("");
                if collection != "app.bsky.feed.post" || rkey.is_empty() {
                    return Err(invalid("Bluesky reply target is invalid"));
                }
                let record_response = self
                    .send(
                        self.request(
                            Method::GET,
                            &format!("{origin}/xrpc/com.atproto.repo.getRecord"),
                            token,
                        )?
                        .query(&[
                            ("repo", repo.as_str()),
                            ("collection", "app.bsky.feed.post"),
                            ("rkey", rkey),
                        ]),
                        false,
                    )
                    .await?;
                let parent_cid = field(&record_response, "cid")?;
                let root_value = &record_response["value"]["reply"]["root"];
                let (root_uri, root_cid) = if root_value.is_object()
                    && root_value["uri"].as_str().is_some()
                    && root_value["cid"].as_str().is_some()
                {
                    (
                        root_value["uri"].as_str().unwrap().to_owned(),
                        root_value["cid"].as_str().unwrap().to_owned(),
                    )
                } else {
                    (item_id.to_owned(), parent_cid.clone())
                };
                let created = chrono::Utc::now().to_rfc3339();
                self.request(
                    Method::POST,
                    &format!("{origin}/xrpc/com.atproto.repo.createRecord"),
                    token,
                )?
                .json(&json!({
                    "repo": c.identity.id,
                    "collection": "app.bsky.feed.post",
                    "record": {
                        "$type": "app.bsky.feed.post",
                        "text": text,
                        "createdAt": created,
                        "reply": {
                            "root": {"uri": root_uri, "cid": root_cid},
                            "parent": {"uri": item_id, "cid": parent_cid}
                        }
                    }
                }))
            }
            "threads" => {
                if text.chars().count() > 500 {
                    return Err(invalid("Threads reply text exceeds the supported length"));
                }
                let account = &c.identity.id;
                let base = "https://graph.threads.net/v1.0";
                let container = self
                    .send(
                        self.request(
                            Method::POST,
                            &format!("{base}/{account}/threads"),
                            token,
                        )?
                        .form(&json!({"media_type":"TEXT","text":text,"reply_to_id":item_id})),
                        true,
                    )
                    .await?;
                let container_id = field(&container, "id")?;
                let published = self
                    .send(
                        self.request(
                            Method::POST,
                            &format!("{base}/{account}/threads_publish"),
                            token,
                        )?
                        .form(&[("creation_id", container_id.as_str())]),
                        true,
                    )
                    .await?;
                let id = field(&published, "id")?;
                return Ok(Step::Published {
                    external_id: id.clone(),
                    external_url: published["permalink"]
                        .as_str()
                        .or_else(|| published["url"].as_str())
                        .map(str::to_owned),
                    receipt: json!({"id":id}),
                });
            }
            // Threads replies need the same persisted container state machine as posts.
            _ => {
                return Err(error(
                    FailureKind::Unavailable,
                    "Reply delivery is not implemented for this provider; open the conversation on the network",
                ));
            }
        };
        let v = self.send(request, true).await?;
        if c.app.provider == "bluesky" {
            if let Some(uri) = v["uri"].as_str() {
                let cid = field(&v, "cid")?;
                let rkey = uri.rsplit('/').next().unwrap_or("");
                if rkey.is_empty() {
                    return Err(error(
                        FailureKind::Uncertain,
                        "Bluesky did not return a usable reply record key",
                    ));
                }
                let url = Some(format!(
                    "https://bsky.app/profile/{}/post/{}",
                    c.identity.id, rkey
                ));
                return Ok(Step::Published {
                    external_id: uri.to_owned(),
                    external_url: url,
                    receipt: json!({"uri":uri,"cid":cid}),
                });
            }
        }
        let id = field(&v, "id")?;
        Ok(Step::Published {
            external_id: id.clone(),
            external_url: v["url"].as_str().map(str::to_owned),
            receipt: json!({"id":id}),
        })
    }
}
