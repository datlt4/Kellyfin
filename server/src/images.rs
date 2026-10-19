//! Pre-rendered WebP variants: images/{kind}/{id}/{s,m,l}.webp (+ orig for custom images).
use anyhow::{bail, Context};
use std::path::{Path, PathBuf};
use tokio::process::Command;

const FILTER: &str = "[0:v]split=4[a][b][c][d];\
    [a]scale='min(400,iw)':-2[s];[b]scale='min(800,iw)':-2[m];[c]scale='min(1280,iw)':-2[l];\
    [d]scale=1:1:flags=area,format=rgb24[px]";

pub fn dir(images: &Path, kind: &str, id: i64) -> PathBuf {
    images.join(kind).join(id.to_string())
}

/// Renders all variants from `input` (ffmpeg input args) into `dst`; returns the average color.
async fn render(input: &[&str], dst: &Path) -> anyhow::Result<String> {
    tokio::fs::create_dir_all(dst).await?;
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y"]).args(input).args(["-filter_complex", FILTER]);
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
    tokio::fs::create_dir_all(dst).await?;
    let orig = dst.join("orig");
    tokio::fs::copy(src, &orig).await.with_context(|| format!("copy {}", src.display()))?;
    render(&["-i", &format!("file:{}", orig.display())], dst).await
}

/// Grabs a frame at 10% of the video (max 5 min in) as thumbnail.
pub async fn from_video(video: &str, duration: f64, dst: &Path) -> anyhow::Result<String> {
    let t = format!("{:.2}", (duration * 0.1).min(300.0));
    render(&["-ss", &t, "-i", &format!("file:{video}")], dst).await
}

/// The best source of an image for export: the custom original, else the largest render.
pub fn export_source(dst: &Path) -> Option<PathBuf> {
    ["orig", "l.webp"].iter().map(|f| dst.join(f)).find(|p| p.exists())
}
