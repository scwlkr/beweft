use super::*;
use std::collections::HashSet;

// Never return a partial selection list as though discovery were complete.
const MAX_PAGES: usize = 100;
const MAX_DESTINATIONS: usize = 10_000;

fn items<'a>(page: &'a Value, field: &str) -> Result<&'a [Value]> {
    if !page.is_object() {
        return Err(invalid("Google returned an invalid discovery page"));
    }
    match page.get(field) {
        None => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        _ => Err(invalid("Google returned an invalid discovery page")),
    }
}

fn next_page(page: &Value, seen: &mut HashSet<String>) -> Result<Option<String>> {
    match page.get("nextPageToken") {
        None => Ok(None),
        Some(Value::String(token)) if token.is_empty() => Ok(None),
        Some(Value::String(token)) if token.len() <= 2048 && seen.insert(token.clone()) => {
            Ok(Some(token.clone()))
        }
        _ => Err(invalid(
            "Google returned an invalid or repeated discovery page token",
        )),
    }
}

impl Adapter {
    pub(super) async fn google_business_identities(
        &self,
        app: &AppConfig,
        tokens: &Tokens,
    ) -> Result<Vec<Channel>> {
        let token = Some(tokens.access_token.as_str());
        let mut accounts = Vec::new();
        let mut page_token = None;
        let mut seen = HashSet::new();
        for page_index in 0..MAX_PAGES {
            let mut request = self
                .request(
                    Method::GET,
                    "https://mybusinessaccountmanagement.googleapis.com/v1/accounts",
                    token,
                )?
                .query(&[("pageSize", "20")]);
            if let Some(value) = page_token.as_deref() {
                request = request.query(&[("pageToken", value)]);
            }
            let page = self.send(request, false).await?;
            for account in items(&page, "accounts")? {
                let parent = field(account, "name")?;
                google_resource(&parent, "accounts")?;
                let name = account["accountName"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&parent);
                accounts.push((parent.clone(), format!("{name} ({parent})")));
            }
            page_token = next_page(&page, &mut seen)?;
            if page_token.is_none() {
                break;
            }
            if page_index + 1 == MAX_PAGES {
                return Err(invalid(
                    "Google account discovery exceeded its page limit; no partial list was saved",
                ));
            }
        }
        let mut result = Vec::new();
        let mut pairs = HashSet::new();
        for (parent, label) in accounts {
            let mut page_token = None;
            let mut seen = HashSet::new();
            for page_index in 0..MAX_PAGES {
                let endpoint = format!(
                    "https://mybusinessbusinessinformation.googleapis.com/v1/{parent}/locations"
                );
                let mut request = self
                    .request(Method::GET, &endpoint, token)?
                    .query(&[("pageSize", "100"), ("readMask", "name,title")]);
                if let Some(value) = page_token.as_deref() {
                    request = request.query(&[("pageToken", value)]);
                }
                let page = self.send(request, false).await?;
                for location in items(&page, "locations")? {
                    let id = field(location, "name")?;
                    google_resource(&id, "locations")?;
                    let name = field(location, "title")?;
                    if !pairs.insert(format!("{parent}/{id}")) || result.len() >= MAX_DESTINATIONS {
                        return Err(invalid(
                            "Google discovery returned duplicate or too many destinations; no partial list was saved",
                        ));
                    }
                    result.push(Channel {
                        app: app.clone(),
                        tokens: tokens.clone(),
                        identity: Identity {
                            id,
                            name,
                            username: Some(label.clone()),
                            parent: Some(parent.clone()),
                        },
                    });
                }
                page_token = next_page(&page, &mut seen)?;
                if page_token.is_none() {
                    break;
                }
                if page_index + 1 == MAX_PAGES {
                    return Err(invalid(
                        "Google location discovery exceeded its page limit; no partial list was saved",
                    ));
                }
            }
        }
        Ok(result)
    }
}
