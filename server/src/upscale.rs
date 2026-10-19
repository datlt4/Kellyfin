//! Experimental AI upscaling. GPU workers (upscaler/) pull jobs from here over HTTP: they announce their models,
//! claim a queued job, download the source, upload the result, and the result becomes an extra version of the video.
use crate::{
    auth::{Admin, User},
    error::{ApiError, ApiResult},
    util, AppState,
};
use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

/// A running job not heard from for this long is given to another worker.
const STALE_SECS: i64 = 600;
const TOKEN_KEY: &str = "upscaler.token";

#[derive(Serialize, Deserialize, Clone)]
pub struct ModelInfo {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Clone)]
struct Worker {
    models: Vec<ModelInfo>,
    seen: Instant,
}

/// Workers seen recently (in memory: they say hello every poll).
#[derive(Default)]
pub struct Workers(Mutex<HashMap<String, Worker>>);

impl Workers {
    fn touch(&self, name: &str, models: Vec<ModelInfo>) {
        self.0.lock().unwrap().insert(name.to_owned(), Worker { models, seen: Instant::now() });
    }

    fn online(&self) -> Vec<WorkerView> {
        let mut m = self.0.lock().unwrap();
        m.retain(|_, w| w.seen.elapsed() < Duration::from_secs(120));
        m.iter()
            .map(|(name, w)| WorkerView { name: name.clone(), models: w.models.clone(), seen_secs_ago: w.seen.elapsed().as_secs() })
            .collect()
    }
}

#[derive(Serialize)]
struct WorkerView {
    name: String,
    models: Vec<ModelInfo>,
    seen_secs_ago: u64,
}

/// The shared secret workers present, created on first use.
async fn token(st: &AppState) -> ApiResult<String> {
    if let Some(t) = crate::settings::get(&st.db, TOKEN_KEY).await {
        return Ok(t);
    }
    let t = util::random_hex(24);
    sqlx::query("INSERT OR IGNORE INTO settings (key, value) VALUES (?, ?)").bind(TOKEN_KEY).bind(&t).execute(&st.db).await?;
    Ok(crate::settings::get(&st.db, TOKEN_KEY).await.unwrap_or(t))
}

/// Worker requests carry `X-Upscaler-Token`.
async fn worker_auth(st: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let sent = headers.get("x-upscaler-token").and_then(|v| v.to_str().ok()).unwrap_or("");
    if sent.is_empty() || sent != token(st).await? {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "bad upscaler token"));
    }
    Ok(())
}

fn output_path(st: &AppState, item: i64, model: &str) -> std::path::PathBuf {
    let safe: String = model.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    st.cfg.data.join("upscaled").join(format!("{item}-{safe}.mp4"))
}

// ---------- Worker API ----------

#[derive(Deserialize)]
pub struct Hello {
    worker: String,
    models: Vec<ModelInfo>,
}

#[derive(Serialize)]
pub struct Claimed {
    job_id: i64,
    item_id: i64,
    model: String,
    duration: Option<f64>,
    width: Option<i64>,
    height: Option<i64>,
}

/// Announces the worker and hands it the oldest job for one of its models (204 when there is none).
pub async fn claim(State(st): State<AppState>, headers: HeaderMap, Json(h): Json<Hello>) -> ApiResult<Response> {
    worker_auth(&st, &headers).await?;
    let names: Vec<String> = h.models.iter().map(|m| m.name.clone()).collect();
    st.upscale_workers.touch(&h.worker, h.models);
    if names.is_empty() {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let now = util::now();
    let marks = vec!["?"; names.len()].join(",");
    let sql = format!(
        "UPDATE upscale_jobs SET status = 'running', worker = ?, progress = 0, error = NULL, updated_at = ?
         WHERE id = (SELECT id FROM upscale_jobs
                     WHERE model IN ({marks}) AND (status = 'queued' OR (status = 'running' AND updated_at < ?))
                     ORDER BY id LIMIT 1)
         RETURNING id, item_id, model"
    );
    let mut q = sqlx::query_as::<_, (i64, i64, String)>(&sql).bind(&h.worker).bind(now);
    for n in &names {
        q = q.bind(n);
    }
    let Some((job_id, item_id, model)) = q.bind(now - STALE_SECS).fetch_optional(&st.db).await? else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    let (duration, width, height): (Option<f64>, Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT duration, width, height FROM items WHERE id = ?").bind(item_id).fetch_one(&st.db).await?;
    tracing::info!("upscale job {job_id} (item {item_id}, {model}) -> {}", h.worker);
    Ok(Json(Claimed { job_id, item_id, model, duration, width, height }).into_response())
}

/// The original video of a claimed job (range requests supported).
pub async fn source(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<i64>, req: Request) -> ApiResult<Response> {
    worker_auth(&st, &headers).await?;
    let path: String = sqlx::query_scalar("SELECT i.path FROM upscale_jobs j JOIN items i ON i.id = j.item_id WHERE j.id = ?")
        .bind(id)
        .fetch_one(&st.db)
        .await?;
    let res = ServeFile::new(path).oneshot(req).await.map_err(|e| anyhow::anyhow!(e))?;
    Ok(res.map(Body::new).into_response())
}

#[derive(Deserialize)]
pub struct Progress {
    progress: f64,
}

pub async fn progress(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<i64>, Json(p): Json<Progress>) -> ApiResult<StatusCode> {
    worker_auth(&st, &headers).await?;
    sqlx::query("UPDATE upscale_jobs SET progress = ?, updated_at = ? WHERE id = ? AND status = 'running'")
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
    sqlx::query("UPDATE upscale_jobs SET status = 'failed', error = ?, updated_at = ? WHERE id = ?")
        .bind(f.error.chars().take(2000).collect::<String>())
        .bind(util::now())
        .bind(id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The upscaled MP4, streamed to disk (it can be gigabytes), then probed and published.
pub async fn result(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<i64>, body: Body) -> ApiResult<StatusCode> {
    worker_auth(&st, &headers).await?;
    let (item_id, model): (i64, String) =
        sqlx::query_as("SELECT item_id, model FROM upscale_jobs WHERE id = ?").bind(id).fetch_one(&st.db).await?;
    let out = output_path(&st, item_id, &model);
    let part = out.with_extension("part");
    tokio::fs::create_dir_all(out.parent().unwrap()).await.map_err(anyhow::Error::from)?;
    let mut f = tokio::fs::File::create(&part).await.map_err(anyhow::Error::from)?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
        f.write_all(&chunk).await.map_err(anyhow::Error::from)?;
    }
    f.flush().await.map_err(anyhow::Error::from)?;
    drop(f);
    tokio::fs::rename(&part, &out).await.map_err(anyhow::Error::from)?;
    let size = dims(&out).await;
    sqlx::query(
        "UPDATE upscale_jobs SET status = 'done', progress = 1, output = ?, width = ?, height = ?, error = NULL, updated_at = ?
         WHERE id = ?",
    )
    .bind(out.to_string_lossy().into_owned())
    .bind(size.map(|s| s.0))
    .bind(size.map(|s| s.1))
    .bind(util::now())
    .bind(id)
    .execute(&st.db)
    .await?;
    tracing::info!("upscale job {id} done: {}", out.display());
    Ok(StatusCode::NO_CONTENT)
}

async fn dims(file: &std::path::Path) -> Option<(i64, i64)> {
    let out = tokio::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0"])
        .arg(format!("file:{}", file.display()))
        .output()
        .await
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let (w, h) = s.trim().split_once(',')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

// ---------- Playback ----------

#[derive(Serialize, sqlx::FromRow)]
pub struct Version {
    model: String,
    width: Option<i64>,
    height: Option<i64>,
}

/// Finished upscaled versions of an item (shown as extra qualities in the player).
pub async fn versions(st: &AppState, item: i64) -> sqlx::Result<Vec<Version>> {
    sqlx::query_as("SELECT model, width, height FROM upscale_jobs WHERE item_id = ? AND status = 'done' ORDER BY model")
        .bind(item)
        .fetch_all(&st.db)
        .await
}

pub async fn play(State(st): State<AppState>, _u: User, Path((id, model)): Path<(i64, String)>, req: Request) -> ApiResult<Response> {
    let out: String = sqlx::query_scalar("SELECT output FROM upscale_jobs WHERE item_id = ? AND model = ? AND status = 'done'")
        .bind(id)
        .bind(&model)
        .fetch_one(&st.db)
        .await?;
    let res = ServeFile::new(out).oneshot(req).await.map_err(|e| anyhow::anyhow!(e))?;
    Ok(res.map(Body::new).into_response())
}

// ---------- Admin ----------

#[derive(Serialize, sqlx::FromRow)]
pub struct JobRow {
    id: i64,
    item_id: i64,
    title: String,
    library: String,
    model: String,
    status: String,
    progress: f64,
    error: Option<String>,
    worker: Option<String>,
    height: Option<i64>,
    updated_at: i64,
}

#[derive(Serialize)]
pub struct Overview {
    token: String,
    workers: Vec<WorkerView>,
    jobs: Vec<JobRow>,
}

pub async fn overview(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<Overview>> {
    let jobs = sqlx::query_as(
        "SELECT j.id, j.item_id, i.title, l.name AS library, j.model, j.status, j.progress, j.error, j.worker, j.height, j.updated_at
         FROM upscale_jobs j JOIN items i ON i.id = j.item_id JOIN libraries l ON l.id = i.library_id
         ORDER BY CASE j.status WHEN 'running' THEN 0 WHEN 'queued' THEN 1 WHEN 'failed' THEN 2 ELSE 3 END, j.id DESC
         LIMIT 500",
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(Overview { token: token(&st).await?, workers: st.upscale_workers.online(), jobs }))
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
    model: String,
    #[serde(default)]
    item_ids: Vec<i64>,
    library_id: Option<i64>,
}

/// Queues the given videos (or a whole library) for a model; failed jobs are retried, finished ones kept.
pub async fn queue(State(st): State<AppState>, _a: Admin, Json(q): Json<Queue>) -> ApiResult<Json<u64>> {
    if q.model.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "model is required"));
    }
    let mut ids = q.item_ids;
    if let Some(lib) = q.library_id {
        ids.extend(sqlx::query_scalar::<_, i64>("SELECT id FROM items WHERE library_id = ?").bind(lib).fetch_all(&st.db).await?);
    }
    let now = util::now();
    let mut added = 0;
    for id in ids {
        added += sqlx::query(
            "INSERT INTO upscale_jobs (item_id, model, created_at, updated_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (item_id, model) DO UPDATE SET status = 'queued', progress = 0, error = NULL, updated_at = excluded.updated_at
             WHERE upscale_jobs.status = 'failed'",
        )
        .bind(id)
        .bind(q.model.trim())
        .bind(now)
        .bind(now)
        .execute(&st.db)
        .await?
        .rows_affected();
    }
    Ok(Json(added))
}

pub async fn delete_job(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let out: Option<Option<String>> =
        sqlx::query_scalar("DELETE FROM upscale_jobs WHERE id = ? RETURNING output").bind(id).fetch_optional(&st.db).await?;
    if let Some(Some(path)) = out {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(StatusCode::NO_CONTENT)
}
