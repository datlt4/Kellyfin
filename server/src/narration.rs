//! Narration (thuyết minh). A worker in the film-sub compose (narrator/) pulls jobs from here. Per video, in four
//! stages: 1. subtitles in the spoken language (the video's own, else Whisper; "auto" detects the language),
//! 2. translation into English, Vietnamese and Chinese (Argos), 3. a voice reads the narration languages (VieNeu for
//! Vietnamese, edge-tts for Chinese), 4. mux: the voice over the ducked original sound. Results live in
//! data/narration/<item>/ (`<lang>.srt`, `<lang>.m4a`): the video files are never touched, and playback adds the
//! narration as an extra audio track (remux / HLS take it as a second input).
use crate::{
    auth::Admin,
    error::{ApiError, ApiResult},
    stream::SubTrack,
    util, AppState,
};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, io::AsyncWriteExt, process::Command};

/// Languages a narration can be made in: (code used in file names and the API, ISO 639-2 tag, label).
const LANGS: &[(&str, &str, &str)] = &[("vi", "vie", "Vietnamese"), ("zh", "zho", "Chinese")];
/// Narration audio shows up as stream index AUDIO_BASE + 1 + position in LANGS (no real file has that many streams).
pub const AUDIO_BASE: i64 = 1000;
/// A running job not heard from for this long is given to another worker.
const STALE_SECS: i64 = 900;
const TOKEN_KEY: &str = "narration.token";
const VOICE_VI_KEY: &str = "narration.voice_vi";
const VOICE_ZH_KEY: &str = "narration.voice_zh";
const DEFAULT_VOICE_VI: &str = "Ngọc Huyền";
const DEFAULT_VOICE_ZH: &str = "zh-CN-XiaoxiaoNeural";
/// Subtitle languages a job can make or read (spoken languages Whisper hears well, plus the targets).
const SUB_LANGS: &[&str] = &["en", "vi", "zh", "ja", "ko", "de", "fr", "es", "it", "pt", "ru", "th", "id", "ms"];
/// Every job translates into these.
const TARGETS: &[&str] = &["en", "vi", "zh"];

pub fn dir(data: &FsPath, item: i64) -> PathBuf {
    data.join("narration").join(item.to_string())
}

/// The narration file behind a virtual audio index, if it was made.
pub fn audio_in(dir: &FsPath, index: i64) -> Option<PathBuf> {
    let pos = usize::try_from(index - AUDIO_BASE - 1).ok()?;
    let (code, ..) = LANGS.get(pos)?;
    Some(dir.join(format!("{code}.m4a"))).filter(|p| p.is_file())
}

pub fn audio_file(data: &FsPath, item: i64, index: i64) -> Option<PathBuf> {
    audio_in(&dir(data, item), index)
}

/// Virtual audio streams for the item page and player, one per finished narration.
pub fn extra_streams(data: &FsPath, item: i64) -> Vec<Value> {
    let d = dir(data, item);
    LANGS
        .iter()
        .enumerate()
        .filter(|(_, (code, ..))| d.join(format!("{code}.m4a")).is_file())
        .map(|(i, (_, iso, name))| {
            json!({"index": AUDIO_BASE + 1 + i as i64, "type": "audio", "codec": "aac", "channels": 2, "lang": iso,
                   "title": format!("Narration ({name})"), "default": false, "forced": false})
        })
        .collect()
}

pub fn sub_file(data: &FsPath, item: i64, lang: &str) -> Option<PathBuf> {
    SUB_LANGS.contains(&lang).then(|| dir(data, item).join(format!("{lang}.srt"))).filter(|p| p.is_file())
}

/// Subtitles the worker made (keys "n<lang>"); only ones the video did not already have are uploaded.
pub fn extra_subs(data: &FsPath, item: i64) -> Vec<SubTrack> {
    SUB_LANGS
        .iter()
        .filter(|l| sub_file(data, item, l).is_some())
        .map(|l| SubTrack {
            key: format!("n{l}"),
            lang: Some((*l).to_owned()),
            title: Some("Kellyfin".into()),
            codec: "srt".into(),
            external: true,
            supported: true,
        })
        .collect()
}

/// "en", "eng", "English", "en-US" -> "en", and likewise for SUB_LANGS. None for anything else.
fn lang_code(lang: &str) -> Option<&'static str> {
    let l = lang.to_ascii_lowercase();
    let l = l.split(['-', '_', '.']).next().unwrap_or("");
    Some(match l {
        "en" | "eng" | "english" => "en",
        "vi" | "vie" | "vn" | "vietnamese" => "vi",
        "zh" | "zho" | "chi" | "chinese" | "cn" => "zh",
        "ja" | "jpn" | "jp" | "japanese" => "ja",
        "ko" | "kor" | "kr" | "korean" => "ko",
        "de" | "deu" | "ger" | "german" => "de",
        "fr" | "fra" | "fre" | "french" => "fr",
        "es" | "spa" | "spanish" => "es",
        "it" | "ita" | "italian" => "it",
        "pt" | "por" | "portuguese" => "pt",
        "ru" | "rus" | "russian" => "ru",
        "th" | "tha" | "thai" => "th",
        "id" | "ind" | "indonesian" => "id",
        "ms" | "msa" | "may" | "malay" => "ms",
        _ => return None,
    })
}

/// A file name a worker may upload: `<SUB_LANGS>.srt` or `<narration lang>.m4a`.
fn upload_ok(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((l, "srt")) => SUB_LANGS.contains(&l),
        Some((l, "m4a")) => LANGS.iter().any(|x| x.0 == l),
        _ => false,
    }
}

/// What a worker last said about the film-sub services it uses (it checks them itself; Kellyfin never calls them).
#[derive(Serialize, Deserialize, Clone)]
pub struct Check {
    name: String,
    ok: bool,
    detail: String,
}

struct Seen {
    at: Instant,
    checks: Vec<Check>,
    voices: Vec<String>,
}

#[derive(Default)]
pub struct Workers(Mutex<HashMap<String, Seen>>);

impl Workers {
    fn touch(&self, h: Hello) {
        self.0.lock().unwrap().insert(h.worker, Seen { at: Instant::now(), checks: h.checks, voices: h.voices });
    }

    fn online(&self) -> Vec<WorkerView> {
        let mut m = self.0.lock().unwrap();
        m.retain(|_, w| w.at.elapsed() < Duration::from_secs(120));
        let mut v: Vec<WorkerView> = m
            .iter()
            .map(|(name, w)| WorkerView {
                name: name.clone(),
                seen_secs_ago: w.at.elapsed().as_secs(),
                checks: w.checks.clone(),
                voices: w.voices.clone(),
            })
            .collect();
        v.sort_by_key(|w| w.seen_secs_ago);
        v
    }
}

#[derive(Serialize)]
struct WorkerView {
    name: String,
    seen_secs_ago: u64,
    /// Service checks from the worker's last poll.
    checks: Vec<Check>,
    /// VieNeu preset voices, for the voice picker.
    voices: Vec<String>,
}

async fn token(st: &AppState) -> ApiResult<String> {
    if let Some(t) = crate::settings::get(&st.db, TOKEN_KEY).await {
        return Ok(t);
    }
    let t = util::random_hex(24);
    sqlx::query("INSERT OR IGNORE INTO settings (key, value) VALUES (?, ?)").bind(TOKEN_KEY).bind(&t).execute(&st.db).await?;
    Ok(crate::settings::get(&st.db, TOKEN_KEY).await.unwrap_or(t))
}

/// Worker requests carry `X-Narrator-Token`.
async fn worker_auth(st: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let sent = headers.get("x-narrator-token").and_then(|v| v.to_str().ok()).unwrap_or("");
    if sent.is_empty() || sent != token(st).await? {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "bad narrator token"));
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    voice_vi: String,
    voice_zh: String,
}

async fn config(st: &AppState) -> Config {
    let get = |k| crate::settings::get(&st.db, k);
    Config {
        voice_vi: get(VOICE_VI_KEY).await.unwrap_or_else(|| DEFAULT_VOICE_VI.into()),
        voice_zh: get(VOICE_ZH_KEY).await.unwrap_or_else(|| DEFAULT_VOICE_ZH.into()),
    }
}

async fn job_item(st: &AppState, id: i64) -> ApiResult<(i64, String, Vec<Value>)> {
    let (item, path, streams): (i64, String, Option<String>) = sqlx::query_as(
        "SELECT i.id, i.path, i.streams FROM narration_jobs j JOIN items i ON i.id = j.item_id WHERE j.id = ?",
    )
    .bind(id)
    .fetch_one(&st.db)
    .await?;
    Ok((item, path, streams.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()))
}

/// The track the narration is mixed over: the default audio track, else the first.
fn source_audio(streams: &[Value]) -> Option<i64> {
    let audio = || streams.iter().filter(|s| s["type"] == "audio");
    audio().find(|s| s["default"] == true).or_else(|| audio().next()).and_then(|s| s["index"].as_i64())
}

// ---------- Worker API ----------

#[derive(Deserialize)]
pub struct Hello {
    worker: String,
    /// The worker's own check of Whisper / Argos / VieNeu / edge-tts, repeated about once a minute.
    #[serde(default)]
    checks: Vec<Check>,
    #[serde(default)]
    voices: Vec<String>,
}

#[derive(Serialize)]
pub struct Claimed {
    job_id: i64,
    item_id: i64,
    title: String,
    duration: Option<f64>,
    /// Spoken language, or "auto".
    source: String,
    /// Subtitles to make by translation (TARGETS).
    targets: Vec<&'static str>,
    /// Narration (voice) languages.
    langs: Vec<String>,
    /// Text subtitles the video already has, by language code -> key for `subtitle`.
    subs: HashMap<String, String>,
    #[serde(flatten)]
    config: Config,
}

/// Hands the worker the oldest queued job (204 when there is none).
pub async fn claim(State(st): State<AppState>, headers: HeaderMap, Json(h): Json<Hello>) -> ApiResult<Response> {
    worker_auth(&st, &headers).await?;
    let worker = h.worker.clone();
    st.narration_workers.touch(h);
    let now = util::now();
    let Some((job_id, item_id, source, langs)): Option<(i64, i64, String, String)> = sqlx::query_as(
        "UPDATE narration_jobs SET status = 'running', worker = ?1, stage = 0, step = 'starting', progress = 0, error = NULL,
           updated_at = ?2
         WHERE id = (SELECT id FROM narration_jobs
                     WHERE status = 'queued' OR (status = 'running' AND updated_at < ?3) ORDER BY id LIMIT 1)
         RETURNING id, item_id, source, langs",
    )
    .bind(&worker)
    .bind(now)
    .bind(now - STALE_SECS)
    .fetch_optional(&st.db)
    .await?
    else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    let (title, duration, path, streams): (String, Option<f64>, String, Option<String>) =
        sqlx::query_as("SELECT title, duration, path, streams FROM items WHERE id = ?").bind(item_id).fetch_one(&st.db).await?;
    let streams: Value = streams.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let tracks = tokio::task::spawn_blocking(move || crate::stream::subtitle_tracks(&path, &streams))
        .await
        .map_err(anyhow::Error::from)?;
    let mut subs = HashMap::new();
    for t in tracks.into_iter().filter(|t| t.supported) {
        if let Some(code) = t.lang.as_deref().and_then(lang_code) {
            subs.entry(code.to_owned()).or_insert(t.key);
        }
    }
    tracing::info!("narration job {job_id} (item {item_id}) -> {worker}");
    let langs = langs.split(',').map(str::to_owned).collect();
    Ok(Json(Claimed { job_id, item_id, title, duration, source, targets: TARGETS.to_vec(), langs, subs, config: config(&st).await })
        .into_response())
}

/// The original sound of a claimed job's video, copied into Matroska (any codec fits).
pub async fn audio(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<i64>) -> ApiResult<Response> {
    worker_auth(&st, &headers).await?;
    let (_, path, streams) = job_item(&st, id).await?;
    let a = source_audio(&streams).ok_or_else(|| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "the video has no audio"))?;
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(format!("file:{path}"))
        .args(["-map", &format!("0:{a}"), "-c", "copy", "-f", "matroska", "pipe:1"])
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(anyhow::Error::from)?;
    let stdout = child.stdout.take().expect("piped stdout");
    // The child lives in the stream state: a dropped connection kills ffmpeg.
    let body = futures_util::stream::unfold((stdout, child), |(mut out, child)| async move {
        let mut buf = vec![0u8; 64 * 1024];
        match out.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, std::io::Error>(axum::body::Bytes::from(buf)), (out, child)))
            }
            Err(e) => Some((Err(e), (out, child))),
        }
    });
    Ok(([(axum::http::header::CONTENT_TYPE, "audio/x-matroska")], Body::from_stream(body)).into_response())
}

/// An existing subtitle of the job's video as SRT.
pub async fn subtitle(State(st): State<AppState>, headers: HeaderMap, Path((id, key)): Path<(i64, String)>) -> ApiResult<Response> {
    worker_auth(&st, &headers).await?;
    let (item, ..) = job_item(&st, id).await?;
    let (input, map, _) = crate::stream::subtitle_source(&st, item, &key).await?;
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(format!("file:{input}"))
        .args(["-map", &map, "-f", "srt", "pipe:1"])
        .kill_on_drop(true)
        .output()
        .await
        .map_err(anyhow::Error::from)?;
    if !out.status.success() {
        return Err(anyhow::anyhow!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim()).into());
    }
    Ok(([(axum::http::header::CONTENT_TYPE, "application/x-subrip; charset=utf-8")], out.stdout).into_response())
}

#[derive(Deserialize)]
pub struct Progress {
    stage: i64,
    step: String,
    progress: f64,
}

pub async fn progress(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<i64>, Json(p): Json<Progress>) -> ApiResult<StatusCode> {
    worker_auth(&st, &headers).await?;
    sqlx::query("UPDATE narration_jobs SET stage = ?, step = ?, progress = ?, updated_at = ? WHERE id = ? AND status = 'running'")
        .bind(p.stage.clamp(0, 4))
        .bind(p.step.chars().take(200).collect::<String>())
        .bind(p.progress.clamp(0.0, 1.0))
        .bind(util::now())
        .bind(id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Failure {
    error: String,
}

pub async fn fail(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<i64>, Json(f): Json<Failure>) -> ApiResult<StatusCode> {
    worker_auth(&st, &headers).await?;
    sqlx::query("UPDATE narration_jobs SET status = 'failed', error = ?, updated_at = ? WHERE id = ?")
        .bind(f.error.chars().take(2000).collect::<String>())
        .bind(util::now())
        .bind(id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// One result file (`FILES`), written next to the others; a partial upload never replaces a finished file.
pub async fn upload(State(st): State<AppState>, headers: HeaderMap, Path((id, name)): Path<(i64, String)>, body: Body) -> ApiResult<StatusCode> {
    worker_auth(&st, &headers).await?;
    if !upload_ok(&name) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "unknown file name"));
    }
    let (item, ..) = job_item(&st, id).await?;
    let d = dir(&st.cfg.data, item);
    tokio::fs::create_dir_all(&d).await.map_err(anyhow::Error::from)?;
    let part = d.join(format!("{name}.part"));
    let mut f = tokio::fs::File::create(&part).await.map_err(anyhow::Error::from)?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
        f.write_all(&chunk).await.map_err(anyhow::Error::from)?;
    }
    f.flush().await.map_err(anyhow::Error::from)?;
    drop(f);
    tokio::fs::rename(&part, d.join(&name)).await.map_err(anyhow::Error::from)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn done(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    worker_auth(&st, &headers).await?;
    sqlx::query("UPDATE narration_jobs SET status = 'done', stage = 4, step = NULL, progress = 1, error = NULL, updated_at = ? WHERE id = ?")
        .bind(util::now())
        .bind(id)
        .execute(&st.db)
        .await?;
    tracing::info!("narration job {id} done");
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Admin ----------

#[derive(Serialize, sqlx::FromRow)]
pub struct JobRow {
    id: i64,
    item_id: i64,
    title: String,
    library: String,
    source: String,
    langs: String,
    status: String,
    stage: i64,
    step: Option<String>,
    progress: f64,
    error: Option<String>,
    worker: Option<String>,
    updated_at: i64,
}

#[derive(Serialize)]
pub struct Overview {
    config: Config,
    token: String,
    workers: Vec<WorkerView>,
    jobs: Vec<JobRow>,
}

pub async fn overview(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<Overview>> {
    let jobs = sqlx::query_as(
        "SELECT j.id, j.item_id, i.title, l.name AS library, j.source, j.langs, j.status, j.stage, j.step, j.progress, j.error,
           j.worker, j.updated_at
         FROM narration_jobs j JOIN items i ON i.id = j.item_id JOIN libraries l ON l.id = i.library_id
         ORDER BY CASE j.status WHEN 'running' THEN 0 WHEN 'queued' THEN 1 WHEN 'failed' THEN 2 ELSE 3 END, j.id DESC
         LIMIT 500",
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(Overview { config: config(&st).await, token: token(&st).await?, workers: st.narration_workers.online(), jobs }))
}

pub async fn save_config(State(st): State<AppState>, _a: Admin, Json(c): Json<Config>) -> ApiResult<StatusCode> {
    for (k, v) in [(VOICE_VI_KEY, c.voice_vi.trim()), (VOICE_ZH_KEY, c.voice_zh.trim())] {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
            .bind(k)
            .bind(v)
            .execute(&st.db)
            .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn new_token(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<String>> {
    let t = util::random_hex(24);
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
        .bind(TOKEN_KEY)
        .bind(&t)
        .execute(&st.db)
        .await?;
    Ok(Json(t))
}

#[derive(Deserialize)]
pub struct Queue {
    #[serde(default)]
    item_ids: Vec<i64>,
    library_id: Option<i64>,
    /// Spoken language of the videos; "auto" (default) detects it.
    #[serde(default)]
    source: Option<String>,
    langs: Vec<String>,
}

/// Queues videos (or a whole library); failed jobs are retried, finished ones kept (delete them to redo).
pub async fn queue(State(st): State<AppState>, _a: Admin, Json(q): Json<Queue>) -> ApiResult<Json<u64>> {
    let langs: Vec<&str> = LANGS.iter().map(|l| l.0).filter(|c| q.langs.iter().any(|x| x == c)).collect();
    if langs.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "pick at least one language"));
    }
    let source = q.source.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("auto");
    if source != "auto" && !SUB_LANGS.contains(&source) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "unknown spoken language"));
    }
    let mut ids = q.item_ids;
    if let Some(lib) = q.library_id {
        ids.extend(sqlx::query_scalar::<_, i64>("SELECT id FROM items WHERE library_id = ? ORDER BY sort_title, id").bind(lib).fetch_all(&st.db).await?);
    }
    let now = util::now();
    let mut added = 0;
    for id in ids {
        added += sqlx::query(
            "INSERT INTO narration_jobs (item_id, source, langs, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT (item_id) DO UPDATE SET status = 'queued', source = excluded.source, langs = excluded.langs, stage = 0,
               step = NULL, progress = 0, error = NULL, updated_at = excluded.updated_at
             WHERE narration_jobs.status = 'failed'",
        )
        .bind(id)
        .bind(source)
        .bind(langs.join(","))
        .bind(now)
        .execute(&st.db)
        .await?
        .rows_affected();
    }
    Ok(Json(added))
}

/// Removes the job and everything it made for the video.
pub async fn delete_job(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let item: Option<i64> = sqlx::query_scalar("DELETE FROM narration_jobs WHERE id = ? RETURNING item_id").bind(id).fetch_optional(&st.db).await?;
    if let Some(item) = item {
        let _ = tokio::fs::remove_dir_all(dir(&st.cfg.data, item)).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_and_virtual_tracks() {
        assert_eq!(lang_code("eng"), Some("en"));
        assert_eq!(lang_code("Vietnamese"), Some("vi"));
        assert_eq!(lang_code("zh-Hans"), Some("zh"));
        assert_eq!(lang_code("kor"), Some("ko"));
        assert_eq!(lang_code("xx"), None);
        assert!(upload_ok("ko.srt") && upload_ok("vi.m4a"));
        assert!(!upload_ok("ko.m4a") && !upload_ok("../x.srt") && !upload_ok("vi.wav"));
        let d = std::env::temp_dir().join(format!("kf-narration-{}", std::process::id()));
        std::fs::create_dir_all(dir(&d, 7)).unwrap();
        std::fs::write(dir(&d, 7).join("zh.m4a"), b"x").unwrap();
        let s = extra_streams(&d, 7);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0]["index"], AUDIO_BASE + 2);
        assert_eq!(s[0]["lang"], "zho");
        assert!(audio_file(&d, 7, AUDIO_BASE + 2).is_some());
        assert!(audio_file(&d, 7, AUDIO_BASE + 1).is_none());
        assert!(audio_file(&d, 7, 1).is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
