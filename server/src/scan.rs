//! Library scan: walk folders -> sync items -> ffprobe new files -> frame-grab missing thumbnails.
use crate::{images, util};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{process::Command, sync::Semaphore, task::JoinSet};
use walkdir::WalkDir;

const WORKERS: usize = 4;

#[derive(Default, Clone, Serialize)]
pub struct Status {
    pub running: bool,
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub error: Option<String>,
    pub finished_at: Option<i64>,
}

#[derive(Default)]
pub struct Scanner(Mutex<Status>);

impl Scanner {
    pub fn status(&self) -> Status {
        self.0.lock().unwrap().clone()
    }

    fn update(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.0.lock().unwrap())
    }

    fn phase(&self, phase: &str, total: usize) {
        self.update(|s| {
            s.phase = phase.into();
            s.done = 0;
            s.total = total;
        })
    }

    fn tick(&self) {
        self.update(|s| s.done += 1)
    }

    /// Starts a background scan; false if one is already running.
    pub fn start(self: &Arc<Self>, db: SqlitePool, images: PathBuf, only: Option<i64>) -> bool {
        {
            let mut s = self.0.lock().unwrap();
            if s.running {
                return false;
            }
            *s = Status { running: true, ..Default::default() };
        }
        let me = self.clone();
        tokio::spawn(async move {
            let r = me.run(&db, &images, only).await;
            me.update(|s| {
                s.running = false;
                s.error = r.err().map(|e| format!("{e:#}"));
                s.finished_at = Some(util::now());
            });
        });
        true
    }

    pub async fn run(&self, db: &SqlitePool, images: &Path, only: Option<i64>) -> anyhow::Result<()> {
        let libs: Vec<(i64, String)> =
            sqlx::query_as("SELECT id, name FROM libraries WHERE ?1 IS NULL OR id = ?1").bind(only).fetch_all(db).await?;
        self.phase("folders", libs.len());
        for (id, name) in &libs {
            sync_library(db, images, *id).await.map_err(|e| e.context(format!("library {name}")))?;
            self.tick();
        }

        let todo: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, path FROM items WHERE probed_at IS NULL AND (?1 IS NULL OR library_id = ?1)",
        )
        .bind(only)
        .fetch_all(db)
        .await?;
        self.phase("probe", todo.len());
        let sem = Arc::new(Semaphore::new(WORKERS));
        let mut set = JoinSet::new();
        for (id, path) in todo {
            let (db, sem) = (db.clone(), sem.clone());
            set.spawn(async move {
                let _p = sem.acquire().await;
                if let Err(e) = probe_item(&db, id, &path).await {
                    tracing::warn!("probe {path}: {e:#}");
                }
            });
        }
        while set.join_next().await.is_some() {
            self.tick();
        }

        let todo: Vec<(i64, String, f64)> = sqlx::query_as(
            "SELECT id, path, duration FROM items
             WHERE image_ver IS NULL AND duration > 0 AND (?1 IS NULL OR library_id = ?1)",
        )
        .bind(only)
        .fetch_all(db)
        .await?;
        self.phase("thumbnails", todo.len());
        for (id, path, duration) in todo {
            let (db, sem, dst) = (db.clone(), sem.clone(), images::dir(images, "item", id));
            set.spawn(async move {
                let _p = sem.acquire().await;
                match images::from_video(&path, duration, &dst).await {
                    Ok(color) => {
                        let _ = sqlx::query("UPDATE items SET image_ver = ?, color = ? WHERE id = ?")
                            .bind(util::now())
                            .bind(color)
                            .bind(id)
                            .execute(&db)
                            .await;
                    }
                    Err(e) => tracing::warn!("thumbnail {path}: {e:#}"),
                }
            });
        }
        while set.join_next().await.is_some() {
            self.tick();
        }
        self.phase("done", 0);
        Ok(())
    }
}

struct FileInfo {
    size: i64,
    mtime: i64,
}

fn walk(roots: &[String]) -> HashMap<String, FileInfo> {
    let mut found = HashMap::new();
    for root in roots {
        let it = WalkDir::new(root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
        for e in it.flatten() {
            if !e.file_type().is_file() || !util::is_video(e.path()) {
                continue;
            }
            let (Some(path), Ok(meta)) = (e.path().to_str(), e.metadata()) else { continue };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            found.insert(path.to_owned(), FileInfo { size: meta.len() as i64, mtime });
        }
    }
    found
}

async fn sync_library(db: &SqlitePool, images: &Path, lib: i64) -> anyhow::Result<()> {
    let roots: Vec<String> =
        sqlx::query_scalar("SELECT path FROM library_paths WHERE library_id = ?").bind(lib).fetch_all(db).await?;
    // A missing root (unmounted disk) must not wipe the library.
    let roots: Vec<String> = roots.into_iter().filter(|r| Path::new(r).is_dir()).collect();
    if roots.is_empty() {
        tracing::warn!("library {lib}: no reachable folder, skipped");
        return Ok(());
    }
    let found = tokio::task::spawn_blocking(move || walk(&roots)).await?;
    let known: Vec<(i64, String, i64, i64)> =
        sqlx::query_as("SELECT id, path, size, mtime FROM items WHERE library_id = ?").bind(lib).fetch_all(db).await?;

    let now = util::now();
    let mut tx = db.begin().await?;
    let mut seen = std::collections::HashSet::new();
    let mut gone = Vec::new();
    for (id, path, size, mtime) in known {
        match found.get(&path) {
            Some(f) => {
                seen.insert(path);
                if f.size != size || f.mtime != mtime {
                    sqlx::query("UPDATE items SET size = ?, mtime = ?, probed_at = NULL WHERE id = ?")
                        .bind(f.size)
                        .bind(f.mtime)
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            None => gone.push(id),
        }
    }
    for id in &gone {
        sqlx::query("DELETE FROM items WHERE id = ?").bind(id).execute(&mut *tx).await?;
    }
    for (path, f) in found.iter().filter(|(p, _)| !seen.contains(*p)) {
        let title = util::file_stem(path);
        sqlx::query(
            "INSERT INTO items (library_id, path, title, sort_title, size, mtime, added_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(lib)
        .bind(path)
        .bind(&title)
        .bind(util::sort_key(&title))
        .bind(f.size)
        .bind(f.mtime)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    for id in gone {
        let _ = tokio::fs::remove_dir_all(images::dir(images, "item", id)).await;
    }
    Ok(())
}

async fn probe_item(db: &SqlitePool, id: i64, path: &str) -> anyhow::Result<()> {
    let out = tokio::time::timeout(
        Duration::from_secs(60),
        Command::new("ffprobe")
            .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
            .arg(format!("file:{path}"))
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    // Mark as probed even on failure so broken files are not retried on every scan.
    let p = if out.status.success() { parse_probe(&serde_json::from_slice(&out.stdout)?) } else { Probe::default() };
    sqlx::query(
        "UPDATE items SET duration = ?, container = ?, width = ?, height = ?, video_codec = ?, bitrate = ?,
         streams = ?, probed_at = ? WHERE id = ?",
    )
    .bind(p.duration)
    .bind(p.container)
    .bind(p.width)
    .bind(p.height)
    .bind(p.video_codec)
    .bind(p.bitrate)
    .bind(p.streams.to_string())
    .bind(util::now())
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

#[derive(Default)]
struct Probe {
    duration: Option<f64>,
    container: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    video_codec: Option<String>,
    bitrate: Option<i64>,
    streams: Value,
}

fn parse_probe(v: &Value) -> Probe {
    let fmt = &v["format"];
    let num = |x: &Value| x.as_str().and_then(|s| s.parse::<f64>().ok());
    let streams: Vec<Value> = v["streams"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|s| s["disposition"]["attached_pic"] != 1)
                .map(|s| {
                    json!({
                        "index": s["index"],
                        "type": s["codec_type"],
                        "codec": s["codec_name"],
                        "profile": s["profile"],
                        "lang": s["tags"]["language"],
                        "title": s["tags"]["title"],
                        "default": s["disposition"]["default"] == 1,
                        "forced": s["disposition"]["forced"] == 1,
                        "width": s["width"],
                        "height": s["height"],
                        "channels": s["channels"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let video = streams.iter().find(|s| s["type"] == "video");
    Probe {
        duration: num(&fmt["duration"]),
        container: fmt["format_name"].as_str().map(str::to_owned),
        width: video.and_then(|s| s["width"].as_i64()),
        height: video.and_then(|s| s["height"].as_i64()),
        video_codec: video.and_then(|s| s["codec"].as_str()).map(str::to_owned),
        bitrate: num(&fmt["bit_rate"]).map(|b| b as i64),
        streams: Value::Array(streams),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffprobe() {
        let v = json!({
            "format": {"duration": "1380.5", "format_name": "mov,mp4", "bit_rate": "900000"},
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "h264", "width": 854, "height": 480, "disposition": {"default": 1}},
                {"index": 1, "codec_type": "audio", "codec_name": "aac", "channels": 2, "tags": {"language": "vie"}},
                {"index": 2, "codec_type": "video", "codec_name": "mjpeg", "disposition": {"attached_pic": 1}}
            ]
        });
        let p = parse_probe(&v);
        assert_eq!(p.duration, Some(1380.5));
        assert_eq!((p.width, p.height), (Some(854), Some(480)));
        assert_eq!(p.video_codec.as_deref(), Some("h264"));
        assert_eq!(p.streams.as_array().unwrap().len(), 2);
        assert_eq!(p.streams[1]["lang"], "vie");
    }
}
