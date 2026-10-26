//! The GitHub REST API as the driver uses it: one [`Api`] call per request, and [`Client`]
//! around it, which waits out rate limits and retries server errors.
//!
//! | Answer | What [`Client::call`] does |
//! |---|---|
//! | 2xx, 3xx, other 4xx | returns it |
//! | 403 or 429 with `x-ratelimit-remaining: 0` | waits until `x-ratelimit-reset` (GitHub's `Date` as now), at most [`Limits::max_wait`], then retries |
//! | 403 or 429 with `retry-after` (secondary limits) | waits that long (at most [`Limits::max_wait`]), then retries |
//! | 5xx, or no answer | retries after 2, 4, 8 … s |
//!
//! After [`Limits::attempts`] attempts the last answer (or error) is returned. The token is the
//! operator's (`gh auth token`), sent only in the `authorization` header and never printed.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::Value;

/// The API root.
pub const API: &str = "https://api.github.com";

/// An HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// GET.
    Get,
    /// POST.
    Post,
    /// PATCH.
    Patch,
    /// DELETE.
    Delete,
}

impl Method {
    /// The method's name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

/// One answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// Status.
    pub status: u16,
    /// The body as JSON (`Null` when empty or not JSON).
    pub json: Value,
    /// `x-ratelimit-remaining`.
    pub remaining: Option<u64>,
    /// `x-ratelimit-reset` (Unix seconds).
    pub reset: Option<u64>,
    /// `retry-after` (seconds).
    pub retry_after: Option<u64>,
    /// GitHub's `Date` (Unix seconds).
    pub date: Option<u64>,
}

impl Response {
    /// A plain answer without rate-limit headers.
    #[must_use]
    pub const fn new(status: u16, json: Value) -> Self {
        Self {
            status,
            json,
            remaining: None,
            reset: None,
            retry_after: None,
            date: None,
        }
    }

    /// Whether the status is 2xx.
    #[must_use]
    pub const fn ok(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// GitHub's message, if the body has one.
    #[must_use]
    pub fn message(&self) -> &str {
        self.json["message"].as_str().unwrap_or("")
    }

    /// How long to wait before retrying, if this answer is a rate limit (see the module docs).
    ///
    /// ```
    /// use ci_bench::github::Response;
    /// use std::time::Duration;
    /// let mut r = Response::new(403, serde_json::Value::Null);
    /// assert_eq!(r.rate_limited(), None, "a plain 403 is final");
    /// r.remaining = Some(0);
    /// r.reset = Some(1_000_060);
    /// r.date = Some(1_000_000);
    /// assert_eq!(r.rate_limited(), Some(Duration::from_secs(61)));
    /// r.retry_after = Some(5);
    /// assert_eq!(r.rate_limited(), Some(Duration::from_secs(5)));
    /// ```
    #[must_use]
    pub fn rate_limited(&self) -> Option<Duration> {
        if !matches!(self.status, 403 | 429) {
            return None;
        }
        if let Some(secs) = self.retry_after {
            return Some(Duration::from_secs(secs));
        }
        match (self.remaining, self.reset) {
            // One second past the reset, measured on GitHub's clock when it sent one.
            (Some(0), Some(reset)) => Some(Duration::from_secs(
                reset.saturating_sub(self.date.unwrap_or(reset)) + 1,
            )),
            // 429 without headers: a minute, as GitHub's docs advise.
            _ if self.status == 429 => Some(Duration::from_mins(1)),
            _ => None,
        }
    }
}

/// Why a call got no answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{method} {path}: {detail}")]
pub struct ApiError {
    /// Method.
    pub method: &'static str,
    /// Path.
    pub path: String,
    /// What failed.
    pub detail: String,
}

/// One request to the API.
pub trait Api {
    /// `method path` (from `/`, query included) with an optional JSON body.
    fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> impl Future<Output = Result<Response, ApiError>>;
}

/// Retry and wait limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Attempts per call.
    pub attempts: u32,
    /// The longest rate-limit wait accepted (a longer one returns the answer).
    pub max_wait: Duration,
    /// The first server-error backoff (doubled per attempt).
    pub backoff: Duration,
}

impl Limits {
    /// Five attempts, rate limits waited up to 15 minutes, 2 s first backoff.
    pub const STANDARD: Self = Self {
        attempts: 5,
        max_wait: Duration::from_mins(15),
        backoff: Duration::from_secs(2),
    };
}

/// [`Api`] with the retry policy of the module docs.
#[derive(Debug, Clone)]
pub struct Client<A> {
    api: A,
    limits: Limits,
}

impl<A: Api> Client<A> {
    /// Wraps `api`.
    pub const fn new(api: A, limits: Limits) -> Self {
        Self { api, limits }
    }

    /// The wrapped API.
    pub const fn api_ref(&self) -> &A {
        &self.api
    }

    /// Unwraps the API.
    pub fn into_api(self) -> A {
        self.api
    }

    /// The call, retried per the module docs.
    ///
    /// # Errors
    /// [`ApiError`] when the last attempt got no answer.
    pub async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Response, ApiError> {
        let mut backoff = self.limits.backoff;
        let mut attempt = 1;
        loop {
            let last = attempt >= self.limits.attempts;
            let result = self.api.send(method, path, body).await;
            // (wait, whether it is a server-error backoff, which doubles).
            let wait = match &result {
                Ok(r) if r.status >= 500 => Some((backoff, true)),
                Ok(r) => match r.rate_limited() {
                    Some(wait) if wait <= self.limits.max_wait => Some((wait, false)),
                    _ => None,
                },
                Err(_) => Some((backoff, true)),
            };
            let Some((wait, doubles)) = wait.filter(|_| !last) else {
                return result;
            };
            tokio::time::sleep(wait).await;
            if doubles {
                backoff = backoff.saturating_mul(2);
            }
            attempt += 1;
        }
    }

    /// The call, which must answer 2xx: its JSON.
    ///
    /// # Errors
    /// No answer, or another status (with GitHub's message).
    pub async fn ok(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, ApiError> {
        let r = self.call(method, path, body).await?;
        if r.ok() {
            Ok(r.json)
        } else {
            Err(ApiError {
                method: method.as_str(),
                path: path.to_owned(),
                detail: format!("answered {}: {}", r.status, r.message()),
            })
        }
    }
}

/// The real API over HTTPS (rustls with ring, the webpki roots), authenticated as the
/// operator.
pub struct Https {
    http: reqwest::Client,
    token: String,
}

impl std::fmt::Debug for Https {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Https { token: <redacted> }")
    }
}

impl Https {
    /// A client with `token`, 10 s to connect and 60 s per request.
    ///
    /// # Errors
    /// TLS setup failed.
    pub fn new(token: String) -> Result<Self, String> {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(true)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_mins(1))
            .user_agent(concat!("ci-bench/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { http, token })
    }

    /// The operator's token from `gh auth token` (the gh CLI's active account).
    ///
    /// # Errors
    /// gh is missing, signed out, or printed nothing.
    pub async fn token_from_gh() -> Result<String, String> {
        let out = tokio::process::Command::new("gh")
            .args(["auth", "token"])
            .output()
            .await
            .map_err(|e| format!("gh auth token: {e}"))?;
        let token = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if !out.status.success() || token.is_empty() {
            return Err("gh auth token printed no token (gh auth login first)".to_owned());
        }
        Ok(token)
    }
}

fn header_u64(headers: &reqwest::header::HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

impl Api for Https {
    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Response, ApiError> {
        let error = |detail: String| ApiError {
            method: method.as_str(),
            path: path.to_owned(),
            detail,
        };
        let m = match method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Patch => reqwest::Method::PATCH,
            Method::Delete => reqwest::Method::DELETE,
        };
        let mut request = self
            .http
            .request(m, format!("{API}{path}"))
            .header("authorization", format!("Bearer {}", self.token))
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", "2022-11-28");
        if let Some(body) = body {
            request = request.json(body);
        }
        let answer = request
            .send()
            .await
            .map_err(|e| error(e.without_url().to_string()))?;
        let headers = answer.headers().clone();
        let date = headers
            .get("date")
            .and_then(|d| d.to_str().ok())
            .and_then(http_date)
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let status = answer.status().as_u16();
        let bytes = answer
            .bytes()
            .await
            .map_err(|e| error(e.without_url().to_string()))?;
        Ok(Response {
            status,
            json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            remaining: header_u64(&headers, "x-ratelimit-remaining"),
            reset: header_u64(&headers, "x-ratelimit-reset"),
            retry_after: header_u64(&headers, "retry-after"),
            date,
        })
    }
}

/// An HTTP `Date` (`Sun, 06 Nov 1994 08:49:37 GMT`).
///
/// ```
/// let t = ci_bench::github::http_date("Mon, 26 Oct 2026 12:00:05 GMT").unwrap();
/// let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
/// assert_eq!(secs, 1_793_016_005);
/// assert!(ci_bench::github::http_date("yesterday").is_none());
/// ```
#[must_use]
pub fn http_date(text: &str) -> Option<SystemTime> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = text.split_whitespace();
    let (_weekday, day, month, year, time) = (
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
    );
    let month = MONTHS.iter().position(|m| *m == month)? + 1;
    humantime::parse_rfc3339(&format!("{year}-{month:02}-{day:0>2}T{time}Z")).ok()
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;

    /// Answers queued per call, in order; records the calls.
    #[derive(Default)]
    pub(crate) struct Queue {
        pub answers: RefCell<VecDeque<Result<Response, ApiError>>>,
        pub calls: RefCell<Vec<(Method, String, Option<Value>)>>,
    }

    impl Api for Queue {
        async fn send(
            &self,
            method: Method,
            path: &str,
            body: Option<&Value>,
        ) -> Result<Response, ApiError> {
            self.calls
                .borrow_mut()
                .push((method, path.to_owned(), body.cloned()));
            self.answers
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| panic!("unexpected call {} {path}", method.as_str()))
        }
    }

    fn limited(reset_in: u64) -> Response {
        Response {
            remaining: Some(0),
            reset: Some(1_000 + reset_in),
            date: Some(1_000),
            ..Response::new(
                403,
                serde_json::json!({"message": "API rate limit exceeded"}),
            )
        }
    }

    #[tokio::test(start_paused = true)]
    async fn waits_out_rate_limits_and_retries_server_errors() {
        let queue = Queue::default();
        queue.answers.borrow_mut().extend([
            Ok(limited(30)),
            Ok(Response::new(502, Value::Null)),
            Err(ApiError {
                method: "GET",
                path: "/x".into(),
                detail: "reset".into(),
            }),
            Ok(Response::new(200, serde_json::json!({"a": 1}))),
        ]);
        let client = Client::new(queue, Limits::STANDARD);
        let start = tokio::time::Instant::now();
        let json = client.ok(Method::Get, "/x", None).await.unwrap();
        assert_eq!(json["a"], 1);
        // 31 s rate limit, then 2 s and 4 s backoffs.
        assert_eq!(start.elapsed(), Duration::from_secs(31 + 2 + 4));
        assert_eq!(client.api.calls.borrow().len(), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_the_attempts_and_on_long_limits() {
        let queue = Queue::default();
        queue
            .answers
            .borrow_mut()
            .extend((0..3).map(|_| Ok(Response::new(500, Value::Null))));
        let limits = Limits {
            attempts: 3,
            ..Limits::STANDARD
        };
        let client = Client::new(queue, limits);
        let r = client.call(Method::Get, "/x", None).await.unwrap();
        assert_eq!(r.status, 500);
        client
            .api_ref()
            .answers
            .borrow_mut()
            .extend((0..3).map(|_| Ok(Response::new(500, Value::Null))));
        let err = client.ok(Method::Get, "/x", None).await.unwrap_err();
        assert!(err.detail.contains("answered 500"), "{err}");

        let queue = Queue::default();
        queue.answers.borrow_mut().push_back(Ok(limited(3600)));
        let client = Client::new(queue, Limits::STANDARD);
        let start = tokio::time::Instant::now();
        let r = client.call(Method::Get, "/x", None).await.unwrap();
        assert_eq!(
            r.status, 403,
            "an hour is beyond max_wait: returned at once"
        );
        assert_eq!(start.elapsed(), Duration::ZERO);
        assert_eq!(client.api.calls.borrow().len(), 1);
    }

    #[test]
    fn final_answers() {
        assert_eq!(Response::new(404, Value::Null).rate_limited(), None);
        assert_eq!(
            Response::new(429, Value::Null).rate_limited(),
            Some(Duration::from_secs(60))
        );
        let mut past = limited(0);
        past.date = None;
        assert_eq!(past.rate_limited(), Some(Duration::from_secs(1)));
        assert!(Response::new(204, Value::Null).ok());
        assert!(!Response::new(301, Value::Null).ok());
        assert_eq!(
            Response::new(422, serde_json::json!({"message": "No ref"})).message(),
            "No ref"
        );
    }
}
