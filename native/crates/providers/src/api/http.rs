//! HTTP as today's `fetch` (Node 24's undici) does it for the providers.
//!
//! What Node's fetch does, and what this repeats:
//!
//! | Behaviour | Node 24 `fetch` | Here |
//! |---|---|---|
//! | Protocol | HTTP/1.1 (undici negotiates no HTTP/2 unless asked) | HTTP/1.1 only (`http1_only`, ALPN `http/1.1`) |
//! | Proxy | none: `HTTP_PROXY`, `HTTPS_PROXY` and the system settings are ignored unless `NODE_USE_ENV_PROXY` is set | none (`no_proxy`): no environment and no system proxy |
//! | Certificates | Node's bundled Mozilla roots (the system store only with `--use-system-ca`, extra roots only with `NODE_EXTRA_CA_CERTS`) | Mozilla's roots from `webpki-roots`, the same list; neither the system store nor `NODE_EXTRA_CA_CERTS` is read, so a machine whose traffic goes through an inspecting proxy fails here as it fails today |
//! | Headers | `host`, `connection: keep-alive`, the caller's in its order, then `accept: */*`, `accept-language: *`, `sec-fetch-mode: cors`, `user-agent: node`, `accept-encoding: gzip, deflate` where the caller set none, then `content-length` | the same names, values and order (a provider sees the same request); header names go out in lower case, where undici keeps the case a caller wrote (`Content-Type`, `Authorization` in the ChatGPT sign-in's requests) |
//! | Compressed answers | gzip and deflate decoded by zlib with `Z_SYNC_FLUSH`: a body cut short is what was decoded, a corrupt one is `terminated` | the same ([`super::decode`], with flate2; `reqwest` decodes nothing) |
//! | Failures | `fetch failed` before the answer's headers, `terminated` when the body is cut, `This operation was aborted` when stopped, `The operation was aborted due to timeout` past the timeout | the same sentences |
//!
//! The crypto provider is `ring` (0.17.14), not `aws-lc-rs`, rustls'
//! default: ring builds from C sources and its crates.io package carries
//! the assembly prebuilt (no NASM, Perl or CMake on any target, BUILDING.md
//! of ring), and needs only a C compiler, clang for
//! `aarch64-pc-windows-msvc`; aws-lc-sys builds AWS-LC's much larger C with
//! a C compiler, and NASM or its prebuilt objects on x86_64 Windows
//! (aws-lc-rs's Windows requirements page). Both are C; ring is the smaller.

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// The sentence of a request that failed before its answer's headers.
pub const FETCH_FAILED: &str = "fetch failed";
/// The sentence of an answer whose body was cut.
pub const TERMINATED: &str = "terminated";
/// The sentence of a request the turn's stop ended (an `AbortController`
/// aborted without a reason).
pub const ABORTED: &str = "This operation was aborted";
/// The sentence of a request past its `AbortSignal.timeout`.
pub const TIMED_OUT: &str = "The operation was aborted due to timeout";

/// A completion's timeout (`llm.js` `request`: four hours).
pub const COMPLETION_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);
/// A model list's timeout (`providerJson`, Google's list: 30 s).
pub const LIST_TIMEOUT: Duration = Duration::from_secs(30);
/// The catalog's timeout (`fetchCatalog`: 10 s).
pub const CATALOG_TIMEOUT: Duration = Duration::from_secs(10);

/// The client every provider shares: HTTP/1.1, no proxy, Mozilla's roots
/// with ring's crypto, gzip and deflate decoded.
pub fn client() -> reqwest::Client {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("ring supports TLS 1.2 and 1.3")
    .with_root_certificates(roots)
    .with_no_client_auth();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .http1_only()
        .no_proxy()
        .build()
        .expect("the HTTP client's settings are valid")
}

/// Why a request ended without its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The connection failed before the answer's headers ([`FETCH_FAILED`]).
    Fetch,
    /// The body was cut ([`TERMINATED`]).
    Terminated,
    /// The turn was stopped ([`ABORTED`]).
    Aborted,
    /// Past the timeout ([`TIMED_OUT`]).
    TimedOut,
}

impl Failure {
    /// Today's sentence for it.
    pub fn message(self) -> &'static str {
        match self {
            Failure::Fetch => FETCH_FAILED,
            Failure::Terminated => TERMINATED,
            Failure::Aborted => ABORTED,
            Failure::TimedOut => TIMED_OUT,
        }
    }
}

/// What ends a request early: the turn's stop and a timeout
/// (`AbortSignal.any([signal, AbortSignal.timeout(ms)])`).
#[derive(Clone, Debug)]
pub struct Deadline {
    cancel: CancellationToken,
    until: Instant,
}

impl Deadline {
    /// `timeout` from now, or sooner if `cancel` is cancelled.
    pub fn new(cancel: &CancellationToken, timeout: Duration) -> Deadline {
        Deadline {
            cancel: cancel.clone(),
            until: Instant::now() + timeout,
        }
    }

    /// Runs `work` until it ends, the stop, or the timeout.
    pub async fn run<T>(&self, work: impl Future<Output = T>) -> Result<T, Failure> {
        if self.cancel.is_cancelled() {
            return Err(Failure::Aborted);
        }
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Failure::Aborted),
            _ = tokio::time::sleep_until(self.until) => Err(Failure::TimedOut),
            out = work => Ok(out),
        }
    }

    /// Whether the turn was stopped.
    pub fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

/// A request as `fetch` would send it.
pub struct Request<'a> {
    /// `GET` or `POST`.
    pub method: reqwest::Method,
    /// The address.
    pub url: &'a str,
    /// The caller's headers, in order (names as undici writes them on the
    /// wire, which is lower case unless the caller wrote otherwise).
    pub headers: Vec<(String, String)>,
    /// The body, if any.
    pub body: Option<String>,
}

/// The headers in the order undici sends them (see the module's table).
fn wire_headers(url: &reqwest::Url, given: &[(String, String)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let host = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().expect("an address with a host")),
        None => url.host_str().expect("an address with a host").to_owned(),
    };
    let mut put = |name: &str, value: &str| {
        headers.append(
            HeaderName::from_bytes(name.as_bytes()).expect("a header name"),
            HeaderValue::from_str(value).expect("a header value"),
        );
    };
    put("host", &host);
    put("connection", "keep-alive");
    for (name, value) in given {
        put(name, value);
    }
    let has = |name: &str| given.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
    for (name, value) in [
        ("accept", "*/*"),
        ("accept-language", "*"),
        ("sec-fetch-mode", "cors"),
        ("user-agent", "node"),
        ("accept-encoding", "gzip, deflate"),
    ] {
        if !has(name) {
            put(name, value);
        }
    }
    headers
}

/// An answer whose headers have come.
pub struct Answer {
    response: reqwest::Response,
    deadline: Deadline,
    decoders: super::decode::Decoders,
    /// The status.
    pub status: u16,
}

/// Sends `request`, until its answer's headers come or `deadline` ends it.
pub async fn send(
    http: &reqwest::Client,
    request: Request<'_>,
    deadline: &Deadline,
) -> Result<Answer, Failure> {
    let url = reqwest::Url::parse(request.url).expect("a provider address is a URL");
    let headers = wire_headers(&url, &request.headers);
    let mut builder = http.request(request.method, url).headers(headers);
    if let Some(body) = request.body {
        builder = builder.body(body);
    }
    let response = deadline
        .run(builder.send())
        .await?
        .map_err(|_| Failure::Fetch)?;
    let status = response.status().as_u16();
    let encoding = response
        .headers()
        .get_all(reqwest::header::CONTENT_ENCODING)
        .iter()
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .collect::<Vec<_>>();
    // Headers.get joins repeated headers with ", ".
    let encoding = (!encoding.is_empty()).then(|| encoding.join(", "));
    Ok(Answer {
        decoders: super::decode::Decoders::new(encoding.as_deref(), status),
        status,
        response,
        deadline: deadline.clone(),
    })
}

impl Answer {
    /// `response.ok`.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The next piece of the body, decoded, or `None` at its end. A body
    /// that is not what its encoding says ends as `terminated`.
    pub async fn chunk(&mut self) -> Result<Option<Vec<u8>>, Failure> {
        let deadline = self.deadline.clone();
        loop {
            let raw = deadline
                .run(self.response.chunk())
                .await?
                .map_err(|_| Failure::Terminated)?;
            let Some(raw) = raw else {
                return Ok(None);
            };
            let decoded = self.decoders.push(&raw).map_err(|_| Failure::Terminated)?;
            if !decoded.is_empty() {
                return Ok(Some(decoded));
            }
        }
    }

    /// The whole body as text (`response.text()`: UTF-8 with U+FFFD for
    /// malformed bytes, a leading byte order mark dropped).
    pub async fn text(mut self) -> Result<String, Failure> {
        let mut bytes = Vec::new();
        while let Some(chunk) = self.chunk().await? {
            bytes.extend_from_slice(&chunk);
        }
        let mut decoder = super::sse::Utf8Decoder::default();
        let mut text = decoder.decode(&bytes);
        text.push_str(&decoder.finish());
        Ok(text)
    }
}
