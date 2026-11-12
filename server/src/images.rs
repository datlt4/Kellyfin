//! Pre-rendered WebP variants: images/{kind}/{id}/{s,m,l}.webp (+ orig for custom images).
use crate::{
    auth::{visible, OptMedia},
    error::{ApiError, ApiResult},
    AppState,
};
use anyhow::{bail, Context};
use axum::{
    body::Body,
    extract::{Path as RoutePath, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tower::ServiceExt;

const FILTER: &str = "split=4[a][b][c][d];\
    [a]scale='min(400,iw)':-2[s];[b]scale='min(800,iw)':-2[m];[c]scale='min(1280,iw)':-2[l];\
    [d]scale=1:1:flags=area,format=rgb24[px]";

pub fn dir(images: &Path, kind: &str, id: i64) -> PathBuf {
    images.join(kind).join(id.to_string())
}

/// Renders all variants from `input` (ffmpeg input args) into `dst`, cropped to `crop` (w:h:x:y) if given;
/// returns the average color.
async fn render(input: &[&str], crop: Option<&str>, dst: &Path) -> anyhow::Result<String> {
    tokio::fs::create_dir_all(dst).await?;
    let crop = crop.filter(|c| !c.is_empty()).map(|c| format!("crop={c},")).unwrap_or_default();
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y"]).args(input).args(["-filter_complex", &format!("[0:v]{crop}{FILTER}")]);
    for v in ["s", "m", "l"] {
        cmd.args(["-map", &format!("[{v}]"), "-frames:v", "1", "-c:v", "libwebp", "-quality", "80"])
            .arg(dst.join(format!("{v}.webp")));
    }
    cmd.args(["-map", "[px]", "-frames:v", "1", "-f", "rawvideo", "pipe:1"]);
    let out = cmd.kill_on_drop(true).output().await.context("spawn ffmpeg")?;
    if !out.status.success() || out.stdout.len() < 3 {
        bail!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let p = &out.stdout;
    Ok(format!("#{:02x}{:02x}{:02x}", p[0], p[1], p[2]))
}

/// Stores a custom image (kept as `orig` for export) and renders it.
pub async fn from_file(src: &Path, dst: &Path) -> anyhow::Result<String> {
    // Decompression-bomb guard: refuse absurd pixel counts before ffmpeg renders.
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0"])
        .arg(format!("file:{}", src.display()))
        .output()
        .await
        .context("spawn ffprobe")?;
    let dims = String::from_utf8_lossy(&out.stdout);
    let (w, h) = dims.trim().split_once(',').and_then(|(w, h)| Some((w.parse::<u64>().ok()?, h.parse::<u64>().ok()?))).unwrap_or((0, 0));
    if w == 0 || h == 0 || w > 16000 || h > 16000 || w * h > 67_000_000 {
        bail!("image dimensions not usable ({w}x{h})");
    }
    tokio::fs::create_dir_all(dst).await?;
    let orig = dst.join("orig");
    tokio::fs::copy(src, &orig).await.with_context(|| format!("copy {}", src.display()))?;
    render(&["-i", &format!("file:{}", orig.display())], None, dst).await
}

/// Thumbnail servir: only images/{item,lib,user}/{id}/{s,m,l}.webp.
/// `<img>` cannot send headers, so the session arrives as `?t=`. Library posters
/// are public; item thumbnails follow the item's restriction; user avatars stay
/// login-only (nothing public links to them).
pub async fn file(State(st): State<AppState>, u: OptMedia, RoutePath(path): RoutePath<String>) -> ApiResult<Response> {
    let bad = || ApiError::new(StatusCode::NOT_FOUND, "not found");
    let mut parts = path.split('/');
    let (kind, id, variant) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(k), Some(id), Some(v), None) => (k, id, v),
        _ => return Err(bad()),
    };
    if !matches!(kind, "item" | "lib" | "user") || !matches!(variant, "s.webp" | "m.webp" | "l.webp") {
        return Err(bad());
    }
    if !id.chars().all(|c| c.is_ascii_digit()) || id.len() > 12 {
        return Err(bad());
    }
    if kind == "user" && u.0.is_none() {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "sign in to view this"));
    }
    if kind == "item" {
        let restricted: Option<bool> =
            sqlx::query_scalar("SELECT restricted FROM items WHERE id = ?").bind(id).fetch_optional(&st.db).await?;
        if let Some(true) = restricted {
            visible(true, &u.0)?;
        }
    }
    let full = st.images.join(kind).join(id).join(variant);
    let req = Request::builder().uri("/").body(Body::empty()).map_err(anyhow::Error::from)?;
    // ServeFile::oneshot never fails (missing files become 404 responses).
    let mut res = tower_http::services::ServeFile::new(full).oneshot(req).await.map(|r| r.map(Body::new).into_response()).unwrap();
    // Immutable only for the bytes themselves: a 401/404 must never be cached
    // (a thumbnail that does not exist yet would stick for a year).
    if res.status().is_success() {
        res.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    } else {
        res.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
    }
    Ok(res)
}

/// Grabs a frame at 10% of the video (max 5 min in) as thumbnail, without its black bars.
pub async fn from_video(video: &str, duration: f64, crop: Option<&str>, dst: &Path) -> anyhow::Result<String> {
    let t = format!("{:.2}", (duration * 0.1).min(300.0));
    render(&["-ss", &t, "-i", &format!("file:{video}")], crop, dst).await
}

/// The best source of an image for export: the custom original, else the largest render.
pub fn export_source(dst: &Path) -> Option<PathBuf> {
    ["orig", "l.webp"].iter().map(|f| dst.join(f)).find(|p| p.exists())
}
