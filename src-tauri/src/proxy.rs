//! Local reverse proxy that adds the server's custom headers (e.g. Pangolin access tokens).
//! <video> and <img> cannot send headers, so in header mode every request goes through
//! http://127.0.0.1:<port>/<secret>/..., streamed both ways (range requests keep working).
use axum::{
    body::Body,
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
    let echo = origin.is_some_and(|o| {
        let o = o.as_bytes();
        o.starts_with(b"tauri://")
            || o.starts_with(b"capacitor://")
            || o.starts_with(b"http://localhost")
            || o.starts_with(b"http://127.0.0.1")
            || o.starts_with(b"http://tauri.")
            || o.starts_with(b"https://tauri.")
            || o.starts_with(b"ionic://")
    });
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
    let mut rb = p.client.request(parts.method, format!("{}{}", t.url.trim_end_matches('/'), rest));
    for (k, v) in parts.headers.iter().filter(|(k, _)| !HOP.contains(&k.as_str()) && *k != header::ORIGIN && *k != header::REFERER) {
        rb = rb.header(k, v);
    }
    for (k, v) in &t.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    // Image uploads top out at 64 MiB server-side; nothing legitimate through
    // the proxy is bigger (whole-video worker uploads bypass the app).
    let body = axum::body::to_bytes(body, 64 << 20).await.map_err(|e| e.to_string())?;
    let up = rb.body(body).send().await.map_err(|e| e.to_string())?;

    let mut res = Response::builder().status(up.status().as_u16());
    for (k, v) in up.headers().iter().filter(|(k, _)| !HOP.contains(&k.as_str()) && !k.as_str().starts_with("access-control-")) {
        res = res.header(k.as_str(), v.as_bytes());
    }
    res.body(Body::from_stream(up.bytes_stream())).map_err(|e| e.to_string())
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
