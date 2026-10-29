use super::*;
impl Adapter {
    /// Bounded recent comment/mention page. Coverage is explicitly partial.
    pub async fn inbox(&self, c: &Channel, external_posts: &[String]) -> Result<Vec<InboxItem>> {
        require_permission(c, "inbox")?;
        let token = Some(c.tokens.access_token.as_str());
        let mut items = vec![];
        match c.app.provider.as_str() {
            "youtube" => {
                let v = self
                    .send(
                        self.request(
                            Method::GET,
                            "https://www.googleapis.com/youtube/v3/commentThreads",
                            token,
                        )?
                        .query(&[
                            ("part", "snippet"),
                            ("allThreadsRelatedToChannelId", c.identity.id.as_str()),
                            ("maxResults", "50"),
                            ("textFormat", "plainText"),
                            ("order", "time"),
                        ]),
                        false,
                    )
                    .await?;
                for t in v["items"].as_array().unwrap_or(&vec![]) {
                    let comment = &t["snippet"]["topLevelComment"];
                    let s = &comment["snippet"];
                    if let Some(id) = comment["id"].as_str() {
                        items.push(InboxItem {
                            reply_context: None,
                            id: id.into(),
                            parent_id: s["videoId"].as_str().map(str::to_owned),
                            author: s["authorDisplayName"]
                                .as_str()
                                .unwrap_or("YouTube viewer")
                                .into(),
                            text: s["textDisplay"].as_str().unwrap_or("").into(),
                            url: s["videoId"]
                                .as_str()
                                .map(|v| format!("https://www.youtube.com/watch?v={v}&lc={id}")),
                            created_at: s["publishedAt"].as_str().map(str::to_owned),
                        });
                    }
                }
            }
            "mastodon" => {
                let v = self
                    .send(
                        self.request(
                            Method::GET,
                            &format!(
                                "{}/api/v1/notifications?types[]=mention&limit=40",
                                instance_origin(&c.app)?
                            ),
                            token,
                        )?,
                        false,
                    )
                    .await?;
                for n in v.as_array().unwrap_or(&vec![]) {
                    let s = &n["status"];
                    if let Some(id) = s["id"].as_str() {
                        let audience_complete = s["account"]["id"].is_string()
                            && s["account"]["acct"].is_string()
                            && s["mentions"].as_array().is_some_and(|mentions| {
                                mentions
                                    .iter()
                                    .all(|a| a["id"].is_string() && a["acct"].is_string())
                            });
                        let mut recipients = vec![];
                        for account in std::iter::once(&s["account"])
                            .chain(s["mentions"].as_array().into_iter().flatten())
                        {
                            if let (Some(id), Some(acct)) =
                                (account["id"].as_str(), account["acct"].as_str())
                                && id != c.identity.id
                                && !recipients.iter().any(|r: &ReplyRecipient| r.id == id)
                            {
                                recipients.push(ReplyRecipient {
                                    id: id.into(),
                                    acct: acct.into(),
                                });
                            }
                        }
                        items.push(InboxItem {
                            reply_context: s["visibility"]
                                .as_str()
                                .filter(|_| audience_complete)
                                .map(|visibility| ReplyContext {
                                    visibility: visibility.into(),
                                    recipients,
                                }),
                            id: id.into(),
                            parent_id: s["in_reply_to_id"].as_str().map(str::to_owned),
                            author: n["account"]["acct"]
                                .as_str()
                                .unwrap_or("Mastodon user")
                                .into(),
                            text: s["content"].as_str().unwrap_or("").into(),
                            url: s["url"].as_str().map(str::to_owned),
                            created_at: s["created_at"].as_str().map(str::to_owned),
                        });
                    }
                }
            }
            "bluesky" => {
                let v = self
                    .send(
                        self.request(
                            Method::GET,
                            &format!(
                                "{}/xrpc/app.bsky.notification.listNotifications?limit=50",
                                instance_origin(&c.app)?
                            ),
                            token,
                        )?,
                        false,
                    )
                    .await?;
                for n in v["notifications"].as_array().unwrap_or(&vec![]) {
                    if !matches!(n["reason"].as_str(), Some("mention" | "reply" | "quote")) {
                        continue;
                    }
                    if let Some(id) = n["uri"].as_str() {
                        items.push(InboxItem {
                            reply_context: None,
                            id: id.into(),
                            parent_id: n["reasonSubject"].as_str().map(str::to_owned),
                            author: n["author"]["handle"]
                                .as_str()
                                .unwrap_or("Bluesky user")
                                .into(),
                            text: n["record"]["text"].as_str().unwrap_or("").into(),
                            url: None,
                            created_at: n["indexedAt"].as_str().map(str::to_owned),
                        });
                    }
                }
            }
            "facebook" | "instagram" | "threads" => {
                for post in external_posts.iter().take(10) {
                    let url = match c.app.provider.as_str() {
                        "facebook" => format!(
                            "https://graph.facebook.com/v23.0/{}/comments?fields=id,message,from,created_time&limit=50",
                            segment(post)
                        ),
                        "instagram" => format!(
                            "https://graph.instagram.com/v23.0/{}/comments?fields=id,text,username,timestamp&limit=50",
                            segment(post)
                        ),
                        _ => format!(
                            "https://graph.threads.net/v1.0/{}/replies?fields=id,text,username,timestamp,permalink&limit=50",
                            segment(post)
                        ),
                    };
                    let v = self
                        .send(self.request(Method::GET, &url, token)?, false)
                        .await?;
                    for n in v["data"].as_array().unwrap_or(&vec![]) {
                        if let Some(id) = n["id"].as_str() {
                            items.push(InboxItem {
                                reply_context: None,
                                id: id.into(),
                                parent_id: Some(post.clone()),
                                author: n["username"]
                                    .as_str()
                                    .or_else(|| n["from"]["name"].as_str())
                                    .unwrap_or("Social account")
                                    .into(),
                                text: n["text"]
                                    .as_str()
                                    .or_else(|| n["message"].as_str())
                                    .unwrap_or("")
                                    .into(),
                                url: n["permalink"].as_str().map(str::to_owned),
                                created_at: n["timestamp"]
                                    .as_str()
                                    .or_else(|| n["created_time"].as_str())
                                    .map(str::to_owned),
                            });
                        }
                    }
                }
            }
            _ => {
                return Err(error(
                    FailureKind::Unavailable,
                    "Inbox reads are not implemented for this provider",
                ));
            }
        }
        Ok(items)
    }
}
