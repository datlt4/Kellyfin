//! Playback: direct file (HTTP range), copy-remux to fragmented MP4 (audio track pick, mkv), subtitles as WebVTT.
use crate::{
    auth::User,
    error::{ApiError, ApiResult},
    util, AppState,
};
use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, Request, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path as FsPath, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};
use tower::ServiceExt;
use tower_http::services::ServeFile;

const SUB_EXT: &[&str] = &["srt", "vtt", "ass", "ssa"];
const TEXT_SUBS: &[&str] = &["subrip", "ass", "ssa", "webvtt", "mov_text", "text"];

async fn item_path(st: &AppState, id: i64) -> ApiResult<String> {
    Ok(sqlx::query_scalar("SELECT path FROM items WHERE id = ?").bind(id).fetch_one(&st.db).await?)
}

/// Original file with range support: zero CPU, instant seeking.
pub async fn file(State(st): State<AppState>, _u: User, Path(id): Path<i64>, req: Request) -> ApiResult<Response> {
    let path = item_path(&st, id).await?;
    let res = ServeFile::new(path).oneshot(req).await.map_err(|e| anyhow::anyhow!(e))?;
    Ok(res.map(Body::new).into_response())
}

#[derive(Deserialize)]
pub struct KeyframeQ {
    at: f64,
}

/// Keyframe at or before `at`; a copy-remux can only start there.
pub async fn keyframe(State(st): State<AppState>, _u: User, Path(id): Path<i64>, Query(q): Query<KeyframeQ>) -> ApiResult<Json<f64>> {
    let path = item_path(&st, id).await?;
    if q.at <= 0.0 {
        return Ok(Json(0.0));
    }
    let out = tokio::time::timeout(
        Duration::from_secs(10),
        Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "v:0", "-skip_frame", "nokey", "-show_entries", "frame=pts_time"])
            .args(["-read_intervals", &format!("{}%+#1", q.at), "-of", "csv=p=0"])
            .arg(format!("file:{path}"))
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("ffprobe timeout"))?
    .map_err(anyhow::Error::from)?;
    let t = String::from_utf8_lossy(&out.stdout).lines().find_map(|l| l.trim().trim_end_matches(',').parse::<f64>().ok());
    Ok(Json(t.filter(|t| *t <= q.at).unwrap_or(q.at)))
}

#[derive(Deserialize)]
pub struct RemuxQ {
    #[serde(default)]
    start: f64,
    /// Absolute stream index of the audio track.
    audio: Option<i64>,
    /// Client can decode the source audio codec inside MP4, so skip the AAC encode.
    #[serde(default)]
    copy_audio: bool,
}

/// Video copied as-is, chosen audio (copied when the client can decode it, else AAC stereo), streamed as fragmented MP4.
pub async fn remux(State(st): State<AppState>, _u: User, Path(id): Path<i64>, Query(q): Query<RemuxQ>) -> ApiResult<Response> {
    let (path, video_codec, streams): (String, Option<String>, Option<String>) =
        sqlx::query_as("SELECT path, video_codec, streams FROM items WHERE id = ?").bind(id).fetch_one(&st.db).await?;
    let streams: Vec<Value> = streams.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let audio = match q.audio {
        Some(i) => streams.iter().find(|s| s["index"] == i && s["type"] == "audio"),
        None => streams.iter().find(|s| s["type"] == "audio"),
    };

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-nostdin"]);
    if q.start > 0.0 {
        cmd.args(["-ss", &format!("{:.3}", q.start)]);
    }
    cmd.arg("-i").arg(format!("file:{path}")).args(["-map", "0:v:0", "-c:v", "copy"]);
    if video_codec.as_deref() == Some("hevc") {
        cmd.args(["-tag:v", "hvc1"]); // Safari only plays hvc1-tagged HEVC
    }
    if let Some(a) = audio {
        cmd.args(["-map", &format!("0:{}", a["index"])]);
        if a["codec"] == "aac" || q.copy_audio {
            cmd.args(["-c:a", "copy"]);
        } else {
            cmd.args(["-c:a", "aac", "-ac", "2", "-b:a", "192k"]);
        }
    }
    cmd.args(["-sn", "-dn", "-avoid_negative_ts", "make_zero", "-f", "mp4"])
        .args(["-movflags", "frag_keyframe+empty_moov+default_base_moof", "pipe:1"])
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(anyhow::Error::from)?;
    let stdout = child.stdout.take().expect("piped stdout");

    // The child lives in the stream state: a client disconnect drops it, which kills ffmpeg.
    let body = futures_util::stream::unfold((stdout, child), |(mut out, child)| async move {
        let mut buf = vec![0u8; 64 * 1024];
        match out.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, std::io::Error>(Bytes::from(buf)), (out, child)))
            }
            Err(e) => Some((Err(e), (out, child))),
        }
    });
    Ok((
        [(header::CONTENT_TYPE, "video/mp4"), (header::CACHE_CONTROL, "no-store")],
        Body::from_stream(body),
    )
        .into_response())
}

/// Size of the MP4 index (moov). Browsers fetch it whole before the first frame.
pub fn mp4_index_size(path: &str) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut off = 0u64;
    for _ in 0..16 {
        let mut h = [0u8; 16];
        f.seek(SeekFrom::Start(off)).ok()?;
        f.read_exact(&mut h[..8]).ok()?;
        let mut size = u32::from_be_bytes(h[..4].try_into().ok()?) as u64;
        if size == 1 {
            f.read_exact(&mut h[8..]).ok()?;
            size = u64::from_be_bytes(h[8..].try_into().ok()?);
        }
        if &h[4..8] == b"moov" {
            return Some(size);
        }
        if size < 8 {
            return None;
        }
        off += size;
    }
    None
}

#[derive(Serialize, Clone)]
pub struct SubTrack {
    /// "e<stream index>" embedded, "x<n>" external file.
    pub key: String,
    pub lang: Option<String>,
    pub title: Option<String>,
    pub codec: String,
    pub external: bool,
    /// Image subtitles (PGS/DVD) need burn-in, not available yet.
    pub supported: bool,
}

/// Subtitle files next to the video: "<stem>.srt", "<stem>.vi.srt", "<stem>.Vietnamese.ass", ...
fn external_subs(video: &str) -> Vec<(std::path::PathBuf, Option<String>, String)> {
    let p = FsPath::new(video);
    let (Some(dir), Some(stem)) = (p.parent(), p.file_stem().and_then(|s| s.to_str())) else { return vec![] };
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut subs: Vec<_> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let rest = name.strip_prefix(stem)?.strip_prefix('.')?;
            let (mid, ext) = rest.rsplit_once('.').map_or(("", rest), |(m, x)| (m, x));
            let ext = ext.to_ascii_lowercase();
            SUB_EXT.contains(&ext.as_str()).then(|| (e.path(), (!mid.is_empty()).then(|| mid.to_owned()), ext))
        })
        .collect();
    subs.sort();
    subs
}

pub fn subtitle_tracks(path: &str, streams: &Value) -> Vec<SubTrack> {
    let mut out: Vec<SubTrack> = external_subs(path)
        .into_iter()
        .enumerate()
        .map(|(n, (_, lang, ext))| SubTrack { key: format!("x{n}"), lang, title: None, codec: ext, external: true, supported: true })
        .collect();
    for s in streams.as_array().into_iter().flatten().filter(|s| s["type"] == "subtitle") {
        let codec = s["codec"].as_str().unwrap_or("?").to_owned();
        out.push(SubTrack {
            key: format!("e{}", s["index"]),
            lang: s["lang"].as_str().filter(|l| *l != "und").map(str::to_owned),
            title: s["title"].as_str().map(str::to_owned),
            supported: TEXT_SUBS.contains(&codec.as_str()),
            codec,
            external: false,
        });
    }
    out
}

/// WebVTT for one subtitle track. Embedded tracks are cached since extracting reads the whole file.
pub async fn subtitle(State(st): State<AppState>, _u: User, Path((id, key)): Path<(i64, String)>) -> ApiResult<Response> {
    let path = item_path(&st, id).await?;
    let key = key.trim_end_matches(".vtt");
    let not_found = || ApiError::new(StatusCode::NOT_FOUND, "no such subtitle");
    let (input, map, cache) = if let Some(n) = key.strip_prefix('x') {
        let n: usize = n.parse().map_err(|_| not_found())?;
        let p = tokio::task::spawn_blocking(move || external_subs(&path)).await.map_err(anyhow::Error::from)?;
        (p.into_iter().nth(n).ok_or_else(not_found)?.0.to_string_lossy().into_owned(), "0:s:0".to_owned(), None)
    } else if let Some(i) = key.strip_prefix('e') {
        let i: i64 = i.parse().map_err(|_| not_found())?;
        (path, format!("0:{i}"), Some(st.cfg.data.join("subs").join(id.to_string()).join(format!("{i}.vtt"))))
    } else {
        return Err(not_found());
    };
    if let Some(c) = &cache {
        if let Ok(v) = tokio::fs::read(c).await {
            return Ok(vtt(v));
        }
    }
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(format!("file:{input}"))
        .args(["-map", &map, "-f", "webvtt", "pipe:1"])
        .kill_on_drop(true)
        .output()
        .await
        .map_err(anyhow::Error::from)?;
    if !out.status.success() {
        return Err(anyhow::anyhow!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim()).into());
    }
    if let Some(c) = cache {
        if let Some(d) = c.parent() {
            let _ = tokio::fs::create_dir_all(d).await;
        }
        let _ = tokio::fs::write(c, &out.stdout).await;
    }
    Ok(vtt(out.stdout))
}

fn vtt(body: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, "text/vtt; charset=utf-8"), (header::CACHE_CONTROL, "private, max-age=300")], body).into_response()
}

#[derive(Deserialize)]
pub struct Progress {
    position: f64,
}

/// Saves the resume point; near the end counts as watched.
pub async fn progress(State(st): State<AppState>, u: User, Path(id): Path<i64>, Json(p): Json<Progress>) -> ApiResult<StatusCode> {
    let duration: Option<f64> = sqlx::query_scalar("SELECT duration FROM items WHERE id = ?").bind(id).fetch_one(&st.db).await?;
    let finished = duration.is_some_and(|d| d > 0.0 && p.position >= d * 0.92);
    sqlx::query(
        "INSERT INTO user_data (user_id, item_id, position, played, last_played) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (user_id, item_id) DO UPDATE SET position = ?3, played = MAX(played, ?4), last_played = ?5",
    )
    .bind(u.id)
    .bind(id)
    .bind(if finished { 0.0 } else { p.position.max(0.0) })
    .bind(finished)
    .bind(util::now())
    .execute(&st.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct UserDataPatch {
    played: Option<bool>,
    favorite: Option<bool>,
}

pub async fn user_data(State(st): State<AppState>, u: User, Path(id): Path<i64>, Json(p): Json<UserDataPatch>) -> ApiResult<StatusCode> {
    sqlx::query(
        "INSERT INTO user_data (user_id, item_id, played, favorite) VALUES (?1, ?2, COALESCE(?3, 0), COALESCE(?4, 0))
         ON CONFLICT (user_id, item_id) DO UPDATE SET played = COALESCE(?3, played), favorite = COALESCE(?4, favorite),
           position = CASE WHEN ?3 IS NOT NULL THEN 0 ELSE position END",
    )
    .bind(u.id)
    .bind(id)
    .bind(p.played)
    .bind(p.favorite)
    .execute(&st.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_external_subs() {
        let d = std::env::temp_dir().join(format!("kf-subs-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for f in ["Ep 1.mp4", "Ep 1.vi.srt", "Ep 1.srt", "Ep 10.en.srt", "Ep 1.nfo"] {
            std::fs::write(d.join(f), "").unwrap();
        }
        let v = d.join("Ep 1.mp4");
        let t = subtitle_tracks(v.to_str().unwrap(), &serde_json::json!([{"index": 2, "type": "subtitle", "codec": "hdmv_pgs_subtitle"}]));
        let got: Vec<_> = t.iter().map(|s| (s.key.as_str(), s.lang.as_deref(), s.supported)).collect();
        assert_eq!(got, vec![("x0", None, true), ("x1", Some("vi"), true), ("e2", None, false)]);
        let mp4 = d.join("a.mp4");
        std::fs::write(&mp4, [&[0, 0, 0, 16][..], b"ftypisom", &[0; 4], &[0, 0, 0, 12], b"moov", &[0; 4]].concat()).unwrap();
        assert_eq!(mp4_index_size(mp4.to_str().unwrap()), Some(12));
        std::fs::remove_dir_all(d).unwrap();
    }
}
