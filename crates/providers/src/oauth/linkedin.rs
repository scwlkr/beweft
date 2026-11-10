use super::*;
use std::collections::{HashMap, HashSet};

const VERSION: &str = "202610";
const COUNT: u64 = 100;
// Never return a partial selection list as though discovery were complete.
const MAX_PAGES: usize = 100;
const MAX_DESTINATIONS: usize = 10_000;
const LOOKUP_BATCH: usize = 50;
const ORGANIZATION_SCOPES: [&str; 2] = ["w_organization_social", "r_organization_admin"];

fn headers(b: RequestBuilder) -> RequestBuilder {
    b.header("Linkedin-Version", VERSION)
        .header("X-Restli-Protocol-Version", "2.0.0")
}

fn acl_error(e: ProviderError) -> ProviderError {
    if e.status == Some(403) {
        error(
            FailureKind::Authentication,
            "LinkedIn denied organization discovery. The app may lack the approved r_organization_admin permission, or this member may not administer an organization. Organization access is optional; reconnect without it to publish as the member.",
        )
    } else {
        e
    }
}

/// Accept both documented ACL identifier fields, but never let a conflict
/// select a different destination.
fn organization_urn(element: &Value) -> Result<String> {
    let from_organization = element.get("organization").and_then(Value::as_str);
    let from_target = element.get("organizationTarget").and_then(Value::as_str);
    let urn = match (from_organization, from_target) {
        (Some(a), Some(b)) if a != b => {
            return Err(invalid(
                "LinkedIn returned conflicting organization identifiers",
            ));
        }
        (Some(a), _) | (_, Some(a)) => a,
        _ => {
            return Err(invalid(
                "LinkedIn returned an organization without an identifier",
            ));
        }
    };
    linkedin_organization_id(urn)
        .ok_or_else(|| invalid("LinkedIn returned an invalid organization identifier"))?;
    Ok(urn.to_owned())
}

fn acl_organization(element: &Value) -> Result<Option<String>> {
    match element.get("role").and_then(Value::as_str) {
        Some("ADMINISTRATOR") => {}
        Some(_) => return Ok(None),
        None => return Err(invalid("LinkedIn returned an organization role without a type")),
    }
    match element.get("state").and_then(Value::as_str) {
        Some("APPROVED") => {}
        Some(_) => return Ok(None),
        None => return Err(invalid("LinkedIn returned an organization role without a state")),
    }
    organization_urn(element).map(Some)
}

fn acl_page(page: &Value, start: u64) -> Result<(&[Value], bool)> {
    if !page.is_object() {
        return Err(invalid("LinkedIn returned an invalid organization page"));
    }
    let elements = match page.get("elements") {
        Some(Value::Array(elements)) => elements.as_slice(),
        None => return Err(invalid("LinkedIn returned an organization page without elements")),
        _ => return Err(invalid("LinkedIn returned an invalid organization page")),
    };
    let paging = page
        .get("paging")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("LinkedIn returned an organization page without paging"))?;
    let next_link = paging
        .get("links")
        .and_then(Value::as_array)
        .is_some_and(|links| {
            links
                .iter()
                .any(|link| link.get("rel").and_then(Value::as_str) == Some("next"))
        });
    let total_more = paging
        .get("total")
        .and_then(Value::as_u64)
        .is_some_and(|total| start.saturating_add(elements.len() as u64) < total);
    // A full page is treated as possibly incomplete even if the provider omits
    // links or a total. The next request then confirms completion.
    Ok((elements, next_link || total_more || elements.len() as u64 >= COUNT))
}

impl Adapter {
    pub(super) async fn linkedin_identities(
        &self,
        app: &AppConfig,
        tokens: &Tokens,
    ) -> Result<Vec<Channel>> {
        let token = Some(tokens.access_token.as_str());
        let member = self
            .send(
                self.request(Method::GET, "https://api.linkedin.com/v2/userinfo", token)?,
                false,
            )
            .await?;
        let member_id = field(&member, "sub")?;
        let member_name = member["name"]
            .as_str()
            .or_else(|| member["display_name"].as_str())
            .unwrap_or(&member_id)
            .to_owned();
        let mut result = vec![Channel {
            app: app.clone(),
            tokens: tokens.clone(),
            identity: Identity {
                parent: None,
                id: member_id.clone(),
                name: member_name,
                username: member["username"].as_str().map(str::to_owned),
            },
        }];
        if !app.organization_access {
            return Ok(result);
        }
        if tokens.scopes_known {
            let missing: Vec<_> = ORGANIZATION_SCOPES
                .iter()
                .filter(|scope| !tokens.scopes.iter().any(|granted| granted == **scope))
                .collect();
            if !missing.is_empty() {
                return Err(error(
                    FailureKind::Authentication,
                    "LinkedIn did not grant the organization scopes this flow requested (w_organization_social and r_organization_admin). Reconnect and grant organization access, or connect without it for member publishing.",
                ));
            }
        }
        let organizations = self.linkedin_organizations(token).await?;
        if organizations.is_empty() {
            return Err(invalid(
                "LinkedIn returned no approved organization where this member is an ADMINISTRATOR. Organization access is optional; reconnect without it to publish as the member.",
            ));
        }
        let names = self.linkedin_organization_names(&organizations, token).await?;
        for urn in organizations {
            let id = linkedin_organization_id(&urn)
                .ok_or_else(|| invalid("LinkedIn returned an invalid organization identifier"))?;
            result.push(Channel {
                app: app.clone(),
                tokens: tokens.clone(),
                identity: Identity {
                    parent: Some(member_id.clone()),
                    name: names.get(id).cloned().unwrap_or_else(|| urn.clone()),
                    id: urn,
                    username: None,
                },
            });
        }
        Ok(result)
    }

    async fn linkedin_organizations(&self, token: Option<&str>) -> Result<Vec<String>> {
        let mut organizations = Vec::new();
        let mut seen = HashSet::new();
        let mut start = 0u64;
        for page_index in 0..MAX_PAGES {
            let url = format!(
                "https://api.linkedin.com/rest/organizationAcls?q=roleAssignee&role=ADMINISTRATOR&state=APPROVED&count={COUNT}&start={start}"
            );
            let page = self
                .send(headers(self.request(Method::GET, &url, token)?), false)
                .await
                .map_err(acl_error)?;
            let (elements, more) = acl_page(&page, start)?;
            for element in elements {
                let Some(urn) = acl_organization(element)? else {
                    continue;
                };
                if seen.insert(urn.clone()) {
                    if organizations.len() >= MAX_DESTINATIONS {
                        return Err(invalid(
                            "LinkedIn organization discovery returned too many destinations; no partial list was saved",
                        ));
                    }
                    organizations.push(urn);
                }
            }
            if !more {
                return Ok(organizations);
            }
            if elements.is_empty() {
                return Err(invalid(
                    "LinkedIn returned an incomplete organization page; no partial list was saved",
                ));
            }
            start = start.saturating_add(elements.len() as u64);
            if page_index + 1 == MAX_PAGES {
                return Err(invalid(
                    "LinkedIn organization discovery exceeded its page limit; no partial list was saved",
                ));
            }
        }
        Err(invalid(
            "LinkedIn organization discovery exceeded its page limit; no partial list was saved",
        ))
    }

    async fn linkedin_organization_names(
        &self,
        organizations: &[String],
        token: Option<&str>,
    ) -> Result<HashMap<String, String>> {
        let mut names = HashMap::new();
        for batch in organizations.chunks(LOOKUP_BATCH) {
            let ids: Vec<&str> = batch
                .iter()
                .filter_map(|urn| linkedin_organization_id(urn))
                .collect();
            if ids.is_empty() {
                continue;
            }
            let url = format!(
                "https://api.linkedin.com/rest/organizationsLookup?ids=List({})",
                ids.join(",")
            );
            let page = match self
                .send(headers(self.request(Method::GET, &url, token)?), false)
                .await
            {
                Ok(page) => page,
                // Names are cosmetic. A denied or missing lookup falls back to
                // the stable URN; authentication and availability failures do
                // not become a silent partial success.
                Err(e)
                    if matches!(
                        e.kind,
                        FailureKind::Authentication
                            | FailureKind::RateLimited
                            | FailureKind::Unavailable
                            | FailureKind::Uncertain
                    ) =>
                {
                    return Err(e);
                }
                Err(_) => continue,
            };
            let Some(results) = page.get("results").and_then(Value::as_object) else {
                continue;
            };
            for id in ids {
                let urn = format!("urn:li:organization:{id}");
                let name = results
                    .get(id)
                    .or_else(|| results.get(&urn))
                    .and_then(|result| result.get("localizedName"))
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty());
                if let Some(name) = name {
                    names.insert(id.to_owned(), name.to_owned());
                }
            }
        }
        Ok(names)
    }
}
