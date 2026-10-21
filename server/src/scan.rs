//! Library scan: walk folders -> sync items -> ffprobe new files -> find black bars -> frame-grab missing thumbnails.
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

        let todo: Vec<(i64, String, f64, i64, i64)> = sqlx::query_as(
            "SELECT id, path, duration, width, height FROM items
             WHERE crop IS NULL AND duration > 0 AND width > 0 AND height > 0 AND (?1 IS NULL OR library_id = ?1)",
        )
        .bind(only)
        .fetch_all(db)
        .await?;
        self.phase("black bars", todo.len());
        for (id, path, duration, w, h) in todo {
            let (db, sem) = (db.clone(), sem.clone());
            set.spawn(async move {
                let _p = sem.acquire().await;
                let crop = detect_crop(&path, duration, w, h).await.unwrap_or_else(|e| {
                    tracing::warn!("cropdetect {path}: {e:#}");
                    String::new()
                });
                // Bars found: grab the thumbnail again without them, unless it is a custom image.
                let _ = sqlx::query(
                    "UPDATE items SET crop = ?1,
                     image_ver = CASE WHEN ?1 <> '' AND image_locked = 0 THEN NULL ELSE image_ver END WHERE id = ?2",
                )
                .bind(crop)
                .bind(id)
                .execute(&db)
                .await;
            });
        }
        while set.join_next().await.is_some() {
            self.tick();
        }

        let todo: Vec<(i64, String, f64, Option<String>)> = sqlx::query_as(
            "SELECT id, path, duration, crop FROM items
             WHERE image_ver IS NULL AND duration > 0 AND (?1 IS NULL OR library_id = ?1)",
        )
        .bind(only)
        .fetch_all(db)
        .await?;
        self.phase("thumbnails", todo.len());
        for (id, path, duration, crop) in todo {
            let (db, sem, dst) = (db.clone(), sem.clone(), images::dir(images, "item", id));
            set.spawn(async move {
                let _p = sem.acquire().await;
                match images::from_video(&path, duration, crop.as_deref(), &dst).await {
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
                    // Replaced file: probe again and grab a new frame unless the image is custom.
                    sqlx::query(
                        "UPDATE items SET size = ?, mtime = ?, probed_at = NULL, crop = NULL,
                         image_ver = CASE WHEN image_locked = 1 THEN image_ver END WHERE id = ?",
                    )
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

/// Black bars around the picture: '' when there are none, else the picture as w:h:x:y.
/// Several points of the video are sampled and their pictures united, so a dark scene cannot over-crop.
async fn detect_crop(path: &str, duration: f64, w: i64, h: i64) -> anyhow::Result<String> {
    let mut found = Vec::new();
    for at in [0.15, 0.35, 0.5, 0.65, 0.85] {
        let out = tokio::time::timeout(
            Duration::from_secs(60),
            Command::new("ffmpeg")
                .args(["-hide_banner", "-nostats", "-ss", &format!("{:.2}", duration * at)])
                .arg("-i")
                .arg(format!("file:{path}"))
                .args(["-map", "0:v:0", "-frames:v", "24", "-vf", "cropdetect=limit=24:round=2:reset=0", "-f", "null", "-"])
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        found.extend(String::from_utf8_lossy(&out.stderr).lines().filter_map(parse_crop).last());
    }
    Ok(unite_crops(&found, w, h))
}

/// The last "crop=w:h:x:y" of a cropdetect log line.
fn parse_crop(line: &str) -> Option<[i64; 4]> {
    let v: Vec<i64> = line.rsplit_once("crop=")?.1.split(':').map(|n| n.trim().parse().ok()).collect::<Option<_>>()?;
    v.try_into().ok()
}

/// Union of the sampled pictures; '' when the bars are under 1% (or every sample was unusable).
fn unite_crops(found: &[[i64; 4]], w: i64, h: i64) -> String {
    // Black frames give empty or negative boxes.
    let ok: Vec<_> = found.iter().filter(|c| c[0] > w / 4 && c[1] > h / 4 && c[2] >= 0 && c[3] >= 0).collect();
    if ok.is_empty() {
        return String::new();
    }
    let x0 = ok.iter().map(|c| c[2]).min().unwrap();
    let y0 = ok.iter().map(|c| c[3]).min().unwrap();
    let x1 = ok.iter().map(|c| c[2] + c[0]).max().unwrap().min(w);
    let y1 = ok.iter().map(|c| c[3] + c[1]).max().unwrap().min(h);
    if (x1 - x0) * 100 >= w * 99 && (y1 - y0) * 100 >= h * 99 {
        return String::new();
    }
    format!("{}:{}:{}:{}", x1 - x0, y1 - y0, x0, y0)
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
                        "tag": s["codec_tag_string"],
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

    #[test]
    fn unites_black_bars() {
        let line = "[Parsed_cropdetect_0 @ 0x55] x1:0 x2:1919 y1:138 y2:941 w:1920 h:800 x:0 y:140 pts:1 t:0.04 limit:0.094 crop=1920:800:0:140";
        assert_eq!(parse_crop(line), Some([1920, 800, 0, 140]));
        assert_eq!(parse_crop("frame=24"), None);
        // A dark sample (smaller box) does not shrink the picture; a black frame is ignored.
        let found = [[1920, 800, 0, 140], [1600, 600, 160, 240], [-1904, -1072, 1912, 1080]];
        assert_eq!(unite_crops(&found, 1920, 1080), "1920:800:0:140");
        // Pillarbox on both sides.
        assert_eq!(unite_crops(&[[1440, 1080, 240, 0]], 1920, 1080), "1440:1080:240:0");
        // Under 1%: nothing to cut.
        assert_eq!(unite_crops(&[[1916, 1076, 2, 2]], 1920, 1080), "");
        assert_eq!(unite_crops(&[], 1920, 1080), "");
    }
}
