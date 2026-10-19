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

fn cors(h: &mut HeaderMap) {
    for (k, v) in [
        ("access-control-allow-origin", "*"),
        ("access-control-allow-headers", "*"),
        ("access-control-allow-methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS"),
        ("access-control-expose-headers", "*"),
    ] {
        h.insert(HeaderName::from_static(k), HeaderValue::from_static(v));
    }
}

impl Proxy {
    pub fn new(secret: String) -> Self {
        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::limited(5)).build().expect("http client");
        Self { target: Arc::new(RwLock::new(None)), secret, client }
    }

    pub fn router(self) -> Router {
        Router::new().fallback(forward).with_state(self)
    }
}

async fn forward(State(p): State<Proxy>, req: Request) -> Response {
    let mut res = handle(&p, req).await.unwrap_or_else(|e| (StatusCode::BAD_GATEWAY, e).into_response());
    cors(res.headers_mut());
    res
}

async fn handle(p: &Proxy, req: Request) -> Result<Response, String> {
    if req.method() == Method::OPTIONS {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let pq = req.uri().path_and_query().map(|x| x.as_str()).unwrap_or("/");
    let rest = pq
        .strip_prefix('/')
        .and_then(|s| s.strip_prefix(p.secret.as_str()))
        .filter(|r| r.is_empty() || r.starts_with('/') || r.starts_with('?'))
        .ok_or("forbidden")?
        .to_owned();
    let Some(t) = p.target.read().unwrap().clone() else { return Err("no server selected".into()) };

    let (parts, body) = req.into_parts();
    let mut rb = p.client.request(parts.method, format!("{}{}", t.url.trim_end_matches('/'), rest));
    for (k, v) in parts.headers.iter().filter(|(k, _)| !HOP.contains(&k.as_str()) && *k != header::ORIGIN && *k != header::REFERER) {
        rb = rb.header(k, v);
    }
    for (k, v) in &t.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    let body = axum::body::to_bytes(body, 128 << 20).await.map_err(|e| e.to_string())?;
    let up = rb.body(body).send().await.map_err(|e| e.to_string())?;

    let mut res = Response::builder().status(up.status().as_u16());
    for (k, v) in up.headers().iter().filter(|(k, _)| !HOP.contains(&k.as_str()) && !k.as_str().starts_with("access-control-")) {
        res = res.header(k.as_str(), v.as_bytes());
    }
    res.body(Body::from_stream(up.bytes_stream())).map_err(|e| e.to_string())
}

/// Binds 127.0.0.1 on a free port and serves the proxy; returns the base URL.
pub fn start(p: Proxy) -> String {
    let std = std::net::TcpListener::bind("127.0.0.1:0").expect("bind proxy");
    std.set_nonblocking(true).expect("nonblocking");
    let base = format!("http://{}/{}", std.local_addr().expect("addr"), p.secret);
    tauri::async_runtime::spawn(async move {
        let listener = tokio::net::TcpListener::from_std(std).expect("tokio listener");
        let _ = axum::serve(listener, p.router()).await;
    });
    base
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
        let r = c.get(format!("{base}/abc/api/x")).header("Range", "bytes=0-1").send().await.unwrap();
        assert_eq!(r.status(), 206);
        assert_eq!(r.headers()["access-control-allow-origin"], "*");
        assert_eq!(r.headers()["content-range"], "bytes 0-1/10");
        assert_eq!(r.text().await.unwrap(), "bytes=0-1");
        // Wrong secret never reaches upstream.
        assert_eq!(c.get(format!("{base}/nope/api/x")).send().await.unwrap().status(), 502);
    }
}
