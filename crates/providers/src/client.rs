use crate::*;
#[derive(Clone)]
pub struct Adapter {
    pub(crate) http: Client,
    pub(crate) fixture_origin: Option<String>,
    pub(crate) deadline: Option<Deadline>,
}
impl Adapter {
    fn default_deadline() -> Result<Deadline> {
        SuspendInstant::now()
            .and_then(|started| started.deadline(Duration::from_secs(60)))
            .ok_or_else(|| {
                error(
                    FailureKind::Unavailable,
                    "Provider elapsed-time clock is unavailable",
                )
            })
    }
    pub fn new() -> Result<Self> {
        Self::build(None)
    }
    /// Explicit fixture-only injection. No production configuration reads this.
    pub fn fixture(origin: &str) -> Result<Self> {
        let u = Url::parse(origin).map_err(|_| invalid("Invalid fixture origin"))?;
        if u.scheme() != "http"
            || !u
                .host_str()
                .is_some_and(|h| h == "127.0.0.1" || h == "[::1]" || h == "::1")
            || u.path() != "/"
            || !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
        {
            return Err(invalid(
                "Fixture origin must be an explicit loopback HTTP origin",
            ));
        }
        Self::build(Some(u.origin().ascii_serialization()))
    }
    pub fn with_resolved_host(host: &str, addresses: &[std::net::SocketAddr]) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(60))
                .connect_timeout(Duration::from_secs(10))
                .user_agent("beweft/0.1")
                .resolve_to_addrs(host, addresses)
                .build()
                .map_err(|_| invalid("HTTP client initialization failed"))?,
            fixture_origin: None,
            deadline: Some(Self::default_deadline()?),
        })
    }
    fn build(fixture_origin: Option<String>) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(60))
                .connect_timeout(Duration::from_secs(10))
                .user_agent("beweft/0.1")
                .build()
                .map_err(|_| invalid("HTTP client initialization failed"))?,
            fixture_origin,
            deadline: Some(Self::default_deadline()?),
        })
    }
    pub(crate) fn endpoint(&self, url: &str) -> Result<String> {
        let u = Url::parse(url).map_err(|_| invalid("Invalid provider endpoint"))?;
        if let Some(origin) = &self.fixture_origin {
            return Ok(format!(
                "{origin}{}{}",
                u.path(),
                u.query().map(|q| format!("?{q}")).unwrap_or_default()
            ));
        }
        if u.scheme() != "https"
            || !u.username().is_empty()
            || u.password().is_some()
            || u.fragment().is_some()
        {
            return Err(invalid(
                "Provider endpoint must use HTTPS without credentials",
            ));
        }
        Ok(url.to_owned())
    }
    pub(crate) fn request(
        &self,
        method: Method,
        url: &str,
        token: Option<&str>,
    ) -> Result<RequestBuilder> {
        let b = self.http.request(method, self.endpoint(url)?);
        Ok(if let Some(t) = token {
            b.bearer_auth(t)
        } else {
            b
        })
    }
    pub(crate) async fn send(&self, b: RequestBuilder, write: bool) -> Result<Value> {
        let response = self.response(b, write).await?;
        self.response_json(response, write).await
    }
    pub(crate) async fn response_json(
        &self,
        response: reqwest::Response,
        write: bool,
    ) -> Result<Value> {
        let body = response.json();
        let value = if let Some(deadline) = self.deadline {
            deadline.run(body).await.map_err(|_| error(FailureKind::Uncertain,
                "Provider response exceeded its time limit; inspect the outcome before retrying"))?
        } else {
            body.await
        };
        value.map_err(|_| {
            error(
                if write {
                    FailureKind::Uncertain
                } else {
                    FailureKind::Rejected
                },
                "Provider returned an invalid response; check the recorded outcome before retrying",
            )
        })
    }
    pub(crate) async fn response(
        &self,
        b: RequestBuilder,
        write: bool,
    ) -> Result<reqwest::Response> {
        let response = self.raw_response(b, write).await?;
        let s = response.status();
        if s.is_success() {
            return Ok(response);
        }
        let kind = if s.as_u16() == 401 {
            FailureKind::Authentication
        } else if s.as_u16() == 429 {
            FailureKind::RateLimited
        } else if s.is_server_error() && write {
            FailureKind::Uncertain
        } else {
            FailureKind::Rejected
        };
        // Do not echo response bodies or request URLs: OAuth errors can contain secrets.
        let message = match kind {
            FailureKind::Authentication => {
                "Provider authorization expired or was revoked. Reconnect this channel."
            }
            FailureKind::RateLimited => "Provider rate limit reached. Wait before retrying.",
            FailureKind::Uncertain => {
                "Provider returned a server error after a write. Reconcile before retrying."
            }
            _ => {
                "Provider rejected the request. Check permissions, account eligibility, format and developer-app access."
            }
        };
        Err(ProviderError {
            kind,
            message: message.into(),
            status: Some(s.as_u16()),
        })
    }
    /// Provider deadlines are derived from the database's remaining time,
    /// never by comparing its timestamp with the application host's clock.
    pub fn with_deadline(mut self, deadline: Deadline) -> Self {
        self.deadline = Some(deadline);
        self
    }
    pub(crate) async fn raw_response(
        &self,
        mut b: RequestBuilder,
        write: bool,
    ) -> Result<reqwest::Response> {
        if let Some(deadline) = self.deadline {
            let remaining = deadline.remaining().ok_or_else(|| error(FailureKind::Uncertain,"The provider operation reached its time limit before the next request; inspect the outcome before retrying"))?;
            b = b.timeout(remaining.min(Duration::from_secs(60)));
        }
        let pending = b.send();
        let response = if let Some(deadline) = self.deadline {
            deadline.run(pending).await.map_err(|_| error(FailureKind::Uncertain,
                "Provider request exceeded its time limit; its outcome may require reconciliation"))?
        } else {
            pending.await
        };
        response.map_err(|_| {
            error(
                if write {
                    FailureKind::Uncertain
                } else {
                    FailureKind::Unavailable
                },
                "Provider request did not complete. Its outcome may require reconciliation.",
            )
        })
    }
}
