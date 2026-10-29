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
            // Threads replies need the same persisted container state machine as posts.
            _ => {
                return Err(error(
                    FailureKind::Unavailable,
                    "Reply delivery is not implemented for this provider; open the conversation on the network",
                ));
            }
        };
        let v = self.send(request, true).await?;
        let id = field(&v, "id")?;
        Ok(Step::Published {
            external_id: id.clone(),
            external_url: v["url"].as_str().map(str::to_owned),
            receipt: json!({"id":id}),
        })
    }
}
