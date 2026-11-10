//! Local reverse proxy that adds the server's custom headers (e.g. Pangolin access tokens).
//! <video> and <img> cannot send headers, so in header mode every request goes through
//! http://127.0.0.1:<port>/<secret>/..., streamed both ways (range requests keep working).
use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct Target {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

#[derive(Clone)]
pub struct Proxy {
    pub target: Arc<RwLock<Option<Target>>>,
    /// Random first path segment, so other apps on the device cannot borrow the headers.
    pub secret: String,
    client: reqwest::Client,
}

/// Hop-by-hop headers are not forwarded.
const HOP: &[&str] = &["connection", "keep-alive", "proxy-connection", "transfer-encoding", "upgrade", "te", "trailer", "host"];

/// Constant-time comparison: the secret's length is public, its content is not.
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for i in 0..a.len() {
        d |= a[i] ^ b[i];
    }
    d == 0
}

/// CORS for exactly one consumer: this app's own webview. The request Origin
/// is echoed only when it is an app origin (tauri/capacitor) or loopback;
/// anything else gets no Allow-Origin, so a stray web page cannot read
/// proxied responses through the user's browser.
fn cors(origin: Option<&HeaderValue>, h: &mut HeaderMap) {
    // Exact (scheme, host) match: prefix matching would also echo back
    // http://localhost.evil.example. Same list as the server's CORS.
    fn allowed(o: &[u8]) -> bool {
        let o = match std::str::from_utf8(o) {
            Ok(o) => o,
            Err(_) => return false,
        };
        let (scheme, rest) = match o.split_once("://") {
            Some(x) => x,
            None => return false,
        };
        // Bracketed IPv6 keeps its colons: [::1]:8080/path -> [::1]. The bracket
        // must close before any host character: [::1].evil.example is rejected.
        let host: &str = match rest.strip_prefix('[') {
            Some(after) => match after.find(']') {
                Some(end) => {
                    let tail = &after[end + 1..];
                    if !(tail.is_empty() || tail.starts_with(['/', '?', ':'])) {
                        return false;
                    }
                    &rest[..end + 2]
                }
                None => return false,
            },
            None => rest.split(['/', '?', ':']).next().unwrap_or(""),
        };
        matches!(
            (scheme, host),
            ("tauri", "localhost")
                | ("capacitor", "localhost")
                | ("ionic", "localhost")
                | ("http", "localhost")
                | ("http", "127.0.0.1")
                | ("http", "[::1]")
                | ("http", "tauri.localhost")
                | ("https", "tauri.localhost")
        )
    }
    let echo = origin.is_some_and(|o| allowed(o.as_bytes()));
    if echo {
        if let Some(v) = origin.and_then(|o| o.to_str().ok()).and_then(|o| HeaderValue::from_str(o).ok()) {
            h.insert(HeaderName::from_static("access-control-allow-origin"), v);
        }
        h.insert(HeaderName::from_static("vary"), HeaderValue::from_static("Origin"));
    }
    for (k, v) in [
        ("access-control-allow-headers", "authorization, content-type, range"),
        ("access-control-allow-methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS"),
    ] {
        h.insert(HeaderName::from_static(k), HeaderValue::from_static(v));
    }
}

impl Proxy {
    pub fn new(secret: String) -> Self {
        // No redirect following: stored access headers must never ride along
        // to wherever a compromised server points a 302.
        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().expect("http client");
        Self { target: Arc::new(RwLock::new(None)), secret, client }
    }

    pub fn router(self) -> Router {
        Router::new().fallback(forward).with_state(self)
    }
}

async fn forward(State(p): State<Proxy>, req: Request) -> Response {
    let origin = req.headers().get(header::ORIGIN).cloned();
    let mut res = handle(&p, req).await.unwrap_or_else(|e| (StatusCode::BAD_GATEWAY, e).into_response());
    cors(origin.as_ref(), res.headers_mut());
    res
}

/// scheme://authority of a URL, for same-origin comparisons.
fn origin_of(url: &str) -> &str {
    match url.split_once("://") {
        Some((s, rest)) => {
            let auth = rest.split('/').next().unwrap_or("");
            &url[..s.len() + 3 + auth.len()]
        }
        None => url,
    }
}

async fn handle(p: &Proxy, req: Request) -> Result<Response, String> {
    if req.method() == Method::OPTIONS {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let pq = req.uri().path_and_query().map(|x| x.as_str()).unwrap_or("/");
    let seg = pq.strip_prefix('/').and_then(|s| s.split(['/', '?']).next()).unwrap_or("");
    if !ct_eq(seg, &p.secret) {
        return Err("forbidden".into());
    }
    let rest = pq[1 + seg.len()..].to_owned();
    if !(rest.is_empty() || rest.starts_with('/') || rest.starts_with('?')) {
        return Err("forbidden".into());
    }
    let Some(t) = p.target.read().unwrap().clone() else { return Err("no server selected".into()) };

    let (parts, body) = req.into_parts();
    // Image uploads top out at 64 MiB server-side; nothing legitimate through
    // the proxy is bigger (whole-video worker uploads bypass the app).
    let body = axum::body::to_bytes(body, 64 << 20).await.map_err(|e| e.to_string())?;
    let mut url = format!("{}{}", t.url.trim_end_matches('/'), rest);
    let first_origin = origin_of(&url).to_owned();
    let stored: Vec<(String, String)> = t.headers.clone();
    let mut method = parts.method.clone();
    let mut send_body: Option<Bytes> = Some(body);
    for _ in 0..6 {
        // Credentials stay on the original origin only: after a redirect
        // elsewhere neither the stored access headers nor the caller's
        // Authorization/Cookie ride along.
        let same_origin = origin_of(&url) == first_origin;
        let mut rb = p.client.request(method.clone(), url.clone());
        for (k, v) in parts.headers.iter().filter(|(k, _)| {
            !HOP.contains(&k.as_str())
                && *k != header::ORIGIN
                && *k != header::REFERER
                && (same_origin || (*k != header::AUTHORIZATION && *k != header::COOKIE && *k != header::PROXY_AUTHORIZATION))
        }) {
            rb = rb.header(k, v);
        }
        if same_origin {
            for (k, v) in &stored {
                rb = rb.header(k.as_str(), v.as_str());
            }
        }
        if let Some(b) = send_body.clone() {
            rb = rb.body(b);
        }
        let up = rb.send().await.map_err(|e| e.to_string())?;
        let status = up.status().as_u16();
        let is_redirect = matches!(status, 301 | 302 | 303 | 307 | 308);
        let loc = up.headers().get(header::LOCATION).and_then(|v| v.to_str().ok()).map(str::to_owned);
        // Redirects are followed by hand (not by reqwest): stored access
        // headers are dropped the moment the origin changes, so they can never
        // ride along to wherever a compromised server points a 302.
        if is_redirect {
            if let Some(next) = loc.as_deref().and_then(|l| resolve(&url, l)) {
                if status == 303 {
                    method = Method::GET;
                    send_body = None;
                }
                url = next;
                continue;
            }
        }
        let mut res = Response::builder().status(status);
        for (k, v) in up.headers().iter().filter(|(k, _)| !HOP.contains(&k.as_str()) && !k.as_str().starts_with("access-control-")) {
            res = res.header(k.as_str(), v.as_bytes());
        }
        return res.body(Body::from_stream(up.bytes_stream())).map_err(|e| e.to_string());
    }
    Err("too many redirects".into())
}

/// Resolves a Location against the current URL (absolute, protocol-relative,
/// or root-/path-relative).
fn resolve(base: &str, loc: &str) -> Option<String> {
    if loc.contains("://") {
        return Some(loc.to_owned());
    }
    if let Some(rest) = loc.strip_prefix("//") {
        let scheme = base.split("://").next().unwrap_or("https");
        return Some(format!("{scheme}://{rest}"));
    }
    let origin = origin_of(base).to_owned();
    if let Some(path) = loc.strip_prefix('/') {
        return Some(format!("{origin}/{path}"));
    }
    // The base query may itself contain '/': cut it (and any fragment) before
    // taking the directory, or "other" would join onto the query string.
    let bare = base.split(['?', '#']).next().unwrap_or(base);
    let dir = bare.rsplit_once('/').map(|(d, _)| d).unwrap_or(&origin);
    Some(format!("{dir}/{loc}"))
}

/// Binds 127.0.0.1 on a free port and serves the proxy; returns the base URL and the serving task.
pub fn start(p: Proxy) -> std::io::Result<(String, tauri::async_runtime::JoinHandle<()>)> {
    let std = std::net::TcpListener::bind("127.0.0.1:0")?;
    std.set_nonblocking(true)?;
    let base = format!("http://{}/{}", std.local_addr()?, p.secret);
    let task = tauri::async_runtime::spawn(async move {
        let listener = tokio::net::TcpListener::from_std(std).expect("tokio listener");
        let _ = axum::serve(listener, p.router()).await;
    });
    Ok((base, task))
}

/// Whether the listener still answers; iOS reclaims a suspended app's listening sockets.
pub async fn alive(base: &str) -> bool {
    let r = reqwest::Client::new().request(Method::OPTIONS, format!("{base}/")).timeout(std::time::Duration::from_secs(1)).send().await;
    matches!(r, Ok(r) if r.status() == StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;

    #[tokio::test]
    async fn forwards_with_headers_and_range() {
        // Upstream that demands the access header and echoes the Range it got.
        let up = Router::new().route(
            "/api/x",
            get(|h: HeaderMap| async move {
                if h.get("p-access-token").map(|v| v.as_bytes()) != Some(b"s3cret") {
                    return (StatusCode::UNAUTHORIZED, "no").into_response();
                }
                let range = h.get(header::RANGE).and_then(|v| v.to_str().ok()).unwrap_or("-").to_owned();
                (StatusCode::PARTIAL_CONTENT, [(header::CONTENT_RANGE, "bytes 0-1/10")], range).into_response()
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let up_url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, up).await.unwrap() });

        let p = Proxy::new("abc".into());
        *p.target.write().unwrap() = Some(Target { url: up_url, headers: vec![("P-Access-Token".into(), "s3cret".into())] });
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, p.router()).await.unwrap() });

        let c = reqwest::Client::new();
        let r = c
            .get(format!("{base}/abc/api/x"))
            .header("Range", "bytes=0-1")
            .header("Origin", "tauri://localhost")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 206);
        assert_eq!(r.headers()["access-control-allow-origin"], "tauri://localhost");
        assert_eq!(r.headers()["content-range"], "bytes 0-1/10");
        assert_eq!(r.text().await.unwrap(), "bytes=0-1");
        // A stray web origin gets no Allow-Origin: its pages cannot read proxied responses.
        let r = c.get(format!("{base}/abc/api/x")).header("Origin", "https://evil.example").send().await.unwrap();
        assert_eq!(r.status(), 206);
        assert!(!r.headers().contains_key("access-control-allow-origin"));
        // Wrong secret never reaches upstream.
        assert_eq!(c.get(format!("{base}/nope/api/x")).send().await.unwrap().status(), 502);
    }

    #[tokio::test]
    async fn redirect_keeps_headers_same_origin_only() {
        // Evil upstream reflects whether it saw the stored header or Authorization.
        let evil = Router::new().route(
            "/probe",
            get(|h: HeaderMap| async move {
                let leak = h.contains_key("p-access-token") || h.contains_key(header::AUTHORIZATION);
                (StatusCode::OK, if leak { "leak" } else { "clean" }).into_response()
            }),
        );
        let el = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let evil_url = format!("http://{}", el.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(el, evil).await.unwrap() });

        let evil_probe = format!("{evil_url}/probe");
        let up = Router::new()
            .route(
                "/api/x",
                get(|h: HeaderMap| async move {
                    if h.get("p-access-token").map(|v| v.as_bytes()) != Some(b"s3cret") {
                        return (StatusCode::UNAUTHORIZED, "no").into_response();
                    }
                    (StatusCode::OK, "x").into_response()
                }),
            )
            // Same-origin redirect: stored headers must survive.
            .route("/redir", get(|| async { (StatusCode::FOUND, [(header::LOCATION, "/api/x")]).into_response() }))
            // Cross-origin redirect: nothing credentialed may ride along.
            .route("/redir-out", get(move || {
                let to = evil_probe.clone();
                async move { (StatusCode::FOUND, [(header::LOCATION, to)]).into_response() }
            }));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let up_url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, up).await.unwrap() });

        let p = Proxy::new("abc".into());
        *p.target.write().unwrap() = Some(Target { url: up_url, headers: vec![("P-Access-Token".into(), "s3cret".into())] });
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, p.router()).await.unwrap() });

        let c = reqwest::Client::new();
        let r = c
            .get(format!("{base}/abc/redir"))
            .header(header::AUTHORIZATION, "Bearer sess")
            .header(header::ORIGIN, "tauri://localhost")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.text().await.unwrap(), "x");
        let r = c
            .get(format!("{base}/abc/redir-out"))
            .header(header::AUTHORIZATION, "Bearer sess")
            .header(header::ORIGIN, "tauri://localhost")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.text().await.unwrap(), "clean");
    }

    #[test]
    fn alive_until_stopped() {
        let (base, task) = start(Proxy::new("abc".into())).unwrap();
        tauri::async_runtime::block_on(async {
            assert!(alive(&base).await);
            task.abort();
            let _ = task.await;
            assert!(!alive(&base).await);
        });
    }
}
