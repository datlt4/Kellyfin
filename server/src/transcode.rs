//! On-demand HLS: segments are transcoded only when the player asks for them.
//! A request far from what any job of the item is producing (seek, quality switch, another viewer) starts a new
//! ffmpeg job; a few jobs per item can run side by side, so two viewers of one video do not cancel each other.
use crate::{
    auth::User,
    error::{ApiError, ApiResult},
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::process::{Child, Command};

/// Target segment length; the real one is a whole number of frames (see `Source::gop`).
const SEGMENT: f64 = 4.0;
/// A request this many segments past the encoder's position restarts it there instead of waiting.
const MAX_WAIT_AHEAD: u32 = 6;
/// Encoder is paused once it is this far ahead of the player, resumed below RESUME_AHEAD.
const PAUSE_AHEAD: u32 = 12;
const RESUME_AHEAD: u32 = 6;
const IDLE_KILL: Duration = Duration::from_secs(90);
/// Jobs kept per item; the least recently used one goes first.
const JOBS_PER_ITEM: usize = 3;

/// (short side, video bitrate kbps)
const LADDER: &[(u32, u32)] = &[(1080, 5000), (720, 2800), (480, 1400), (360, 800), (240, 450), (144, 250)];

/// Hardware encoder; None everywhere means x264 on the CPU.
#[derive(Clone, Debug)]
pub enum Hw {
    /// AMD / Intel through a DRM render node.
    Vaapi(String),
    /// NVIDIA (needs the NVIDIA container toolkit with the "video" capability).
    Nvenc,
}

impl Hw {
    fn name(hw: Option<&Hw>) -> &'static str {
        match hw {
            Some(Hw::Vaapi(_)) => "vaapi",
            Some(Hw::Nvenc) => "nvenc",
            None => "x264",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Variant {
    p: u32,
    audio: i64,
}

struct Job {
    item: i64,
    variant: Variant,
    start: u32,
    /// Highest finished segment index (start - 1 when none yet).
    produced: i64,
    last_seg: u32,
    last_req: Instant,
    paused: bool,
    hw: bool,
    encoder: &'static str,
    seg_len: f64,
    child: Child,
    dir: PathBuf,
    generation: u64,
}

impl Job {
    fn pid(&self) -> Option<i32> {
        self.child.id().map(|p| p as i32)
    }

    fn signal(&mut self, pause: bool) {
        if self.paused == pause {
            return;
        }
        if let Some(pid) = self.pid() {
            unsafe { libc::kill(pid, if pause { libc::SIGSTOP } else { libc::SIGCONT }) };
        }
        self.paused = pause;
    }

    fn refresh_produced(&mut self) {
        while self.dir.join(format!("{}.ts", self.produced + 1)).exists() {
            self.produced += 1;
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // A stopped process ignores SIGKILL until continued on some kernels; resume first.
        self.signal(false);
        let _ = self.child.start_kill();
        let dir = self.dir.clone();
        tokio::spawn(async move {
            let _ = tokio::fs::remove_dir_all(dir).await;
        });
    }
}

#[derive(Serialize, Clone)]
pub struct JobInfo {
    pub item_id: i64,
    pub p: u32,
    pub audio: i64,
    pub encoder: &'static str,
    pub position: f64,
    pub paused: bool,
}

pub struct Transcoder {
    /// By generation (unique per job).
    jobs: Mutex<HashMap<u64, Job>>,
    fps: Mutex<HashMap<i64, f64>>,
    root: PathBuf,
    next_gen: Mutex<u64>,
}

struct Source {
    path: String,
    width: u32,
    height: u32,
    duration: f64,
    fps: f64,
    streams: Vec<Value>,
}

impl Source {
    /// Frames per segment. A fixed GOP is the only keyframe control h264_vaapi honours,
    /// so segments are exactly `gop` frames long.
    fn gop(&self) -> u32 {
        (self.fps * SEGMENT).round().max(1.0) as u32
    }

    fn seg_len(&self) -> f64 {
        self.gop() as f64 / self.fps
    }
}

/// Output size for a short-side target, keeping aspect and even dimensions (portrait too).
fn target_size(w: u32, h: u32, p: u32) -> (u32, u32) {
    let even = |x: f64| ((x / 2.0).round() as u32 * 2).max(2);
    let short = w.min(h);
    let p = p.min(short);
    if w >= h {
        (even(w as f64 * p as f64 / h as f64), even(p as f64))
    } else {
        (even(p as f64), even(h as f64 * p as f64 / w as f64))
    }
}

/// Ladder rungs for a source: every standard size below it plus the source size itself (capped at 1080).
fn ladder(w: u32, h: u32) -> Vec<(u32, u32)> {
    let short = w.min(h).min(1080);
    let mut out: Vec<(u32, u32)> = LADDER.iter().copied().filter(|(p, _)| *p < short).collect();
    let top = LADDER.iter().rev().find(|(p, _)| *p >= short).map_or(5000, |r| r.1);
    out.insert(0, (short, top));
    out
}

fn bitrate(p: u32) -> u32 {
    LADDER.iter().rev().find(|(lp, _)| *lp >= p).map_or(5000, |r| r.1)
}

impl Transcoder {
    pub fn new(root: PathBuf) -> Arc<Self> {
        let _ = std::fs::remove_dir_all(&root);
        Arc::new(Self { jobs: Mutex::new(HashMap::new()), fps: Mutex::new(HashMap::new()), root, next_gen: Mutex::new(0) })
    }

    pub fn list(&self) -> Vec<JobInfo> {
        self.jobs
            .lock()
            .unwrap()
            .values()
            .map(|j| JobInfo {
                item_id: j.item,
                p: j.variant.p,
                audio: j.variant.audio,
                encoder: j.encoder,
                position: j.last_seg as f64 * j.seg_len,
                paused: j.paused,
            })
            .collect()
    }

    fn spawn(self: &Arc<Self>, item: i64, src: &Source, v: Variant, start: u32, hw: Option<&Hw>) -> anyhow::Result<Job> {
        let generation = {
            let mut g = self.next_gen.lock().unwrap();
            *g += 1;
            *g
        };
        let dir = self.root.join(format!("{item}-{generation}"));
        std::fs::create_dir_all(&dir)?;
        let (w, h) = target_size(src.width, src.height, v.p);
        let kbps = bitrate(v.p);
        let (gop, seg_len) = (src.gop(), src.seg_len());
        let at = start as f64 * seg_len;

        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin"]);
        match hw {
            // Decoding stays on the CPU: on Vega it beats VAAPI decode (measured 3.9s vs 6.0s per 20s of HEVC 1080p).
            Some(Hw::Vaapi(dev)) => {
                cmd.args(["-init_hw_device", &format!("vaapi=va:{dev}"), "-filter_hw_device", "va"]);
            }
            // NVDEC decodes, frames come back to system memory for scaling (falls back to CPU decode by itself).
            Some(Hw::Nvenc) => {
                cmd.args(["-hwaccel", "cuda"]);
            }
            None => {}
        }
        // copyts keeps source timestamps, so segments from different runs line up on one timeline.
        cmd.args(["-ss", &format!("{at:.3}"), "-copyts", "-start_at_zero"])
            .arg("-i")
            .arg(format!("file:{}", src.path))
            .args(["-map", "0:v:0"]);
        if v.audio >= 0 {
            cmd.args(["-map", &format!("0:{}", v.audio), "-c:a", "aac", "-ac", "2", "-b:a", "160k"]);
        }
        match hw {
            Some(Hw::Vaapi(_)) => cmd.args([
                "-vf",
                &format!("format=nv12,hwupload,scale_vaapi=w={w}:h={h}:format=nv12"),
                "-c:v",
                "h264_vaapi",
            ]),
            Some(Hw::Nvenc) => cmd.args([
                "-vf",
                &format!("scale={w}:{h},format=yuv420p"),
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p4",
                "-forced-idr",
                "1",
            ]),
            None => cmd.args([
                "-vf",
                &format!("scale={w}:{h},format=yuv420p"),
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-sc_threshold",
                "0",
            ]),
        };
        cmd.args(["-b:v", &format!("{kbps}k"), "-maxrate", &format!("{}k", kbps * 3 / 2), "-bufsize", &format!("{}k", kbps * 2)])
            .args(["-g", &gop.to_string(), "-keyint_min", &gop.to_string()])
            .args(["-sn", "-dn", "-muxdelay", "0", "-f", "hls", "-hls_time", &format!("{seg_len:.6}")])
            .args(["-hls_list_size", "0", "-hls_flags", "temp_file", "-start_number", &start.to_string()])
            .arg("-hls_segment_filename")
            .arg(dir.join("%d.ts"))
            .arg(dir.join("ff.m3u8"))
            .stdout(Stdio::null())
            // Into the container log; a pipe nobody reads would stall ffmpeg once full.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let child = cmd.spawn()?;
        tracing::info!("transcode item {item} {}p audio {} from {at}s ({})", v.p, v.audio, Hw::name(hw));

        let job = Job {
            item,
            variant: v,
            start,
            produced: start as i64 - 1,
            last_seg: start,
            last_req: Instant::now(),
            paused: false,
            hw: hw.is_some(),
            encoder: Hw::name(hw),
            seg_len,
            child,
            dir,
            generation,
        };
        let me = self.clone();
        tokio::spawn(async move { me.watch(generation).await });
        Ok(job)
    }

    /// Throttles a job to stay a little ahead of the player and reaps it once idle.
    async fn watch(self: Arc<Self>, generation: u64) {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let mut jobs = self.jobs.lock().unwrap();
            let Some(j) = jobs.get_mut(&generation) else { return };
            if j.last_req.elapsed() > IDLE_KILL {
                jobs.remove(&generation);
                return;
            }
            j.refresh_produced();
            let ahead = j.produced - j.last_seg as i64;
            if ahead >= PAUSE_AHEAD as i64 {
                j.signal(true);
            } else if ahead < RESUME_AHEAD as i64 {
                j.signal(false);
            }
        }
    }

    /// Returns the path of segment `n`, starting or restarting the encoder as needed.
    async fn segment(self: &Arc<Self>, item: i64, src: &Source, v: Variant, n: u32, hw: Option<Hw>) -> anyhow::Result<PathBuf> {
        let mut generation;
        {
            let mut jobs = self.jobs.lock().unwrap();
            let near = jobs.values_mut().filter(|j| j.item == item && j.variant == v && n >= j.start).find_map(|j| {
                j.refresh_produced();
                (n as i64 <= j.produced + MAX_WAIT_AHEAD as i64).then_some(j.generation)
            });
            generation = match near {
                Some(g) => g,
                None => {
                    let mine: Vec<(Instant, u64)> =
                        jobs.values().filter(|j| j.item == item).map(|j| (j.last_req, j.generation)).collect();
                    if mine.len() >= JOBS_PER_ITEM {
                        let oldest = mine.iter().min().unwrap().1;
                        jobs.remove(&oldest);
                    }
                    let job = self.spawn(item, src, v, n, hw.as_ref())?;
                    let g = job.generation;
                    jobs.insert(g, job);
                    g
                }
            };
            let j = jobs.get_mut(&generation).unwrap();
            j.last_req = Instant::now();
            j.last_seg = n;
            if n as i64 > j.produced {
                j.signal(false);
            }
        }

        let deadline = Instant::now() + Duration::from_secs(45);
        let mut retried_sw = false;
        loop {
            let (path, exited) = {
                let mut jobs = self.jobs.lock().unwrap();
                let Some(j) = jobs.get_mut(&generation) else {
                    anyhow::bail!("superseded");
                };
                let path = j.dir.join(format!("{n}.ts"));
                if path.exists() {
                    return Ok(path);
                }
                let exited = match j.child.try_wait() {
                    Ok(Some(st)) => Some((st.success(), j.hw, j.produced < j.start as i64)),
                    _ => None,
                };
                (path, exited)
            };
            if let Some((ok, was_hw, nothing_made)) = exited {
                if path.exists() {
                    return Ok(path);
                }
                // VAAPI can fail on odd inputs: retry once in software.
                if !ok && was_hw && nothing_made && !retried_sw {
                    retried_sw = true;
                    tracing::warn!("{} transcode failed for item {item}, retrying with x264", Hw::name(hw.as_ref()));
                    let mut jobs = self.jobs.lock().unwrap();
                    jobs.remove(&generation);
                    let job = self.spawn(item, src, v, n, None)?;
                    generation = job.generation;
                    jobs.insert(generation, job);
                    continue;
                }
                anyhow::bail!("transcoder exited before segment {n}");
            }
            if Instant::now() > deadline {
                anyhow::bail!("timed out waiting for segment {n}");
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    }
}

/// Average frame rate of the video stream, cached per item (one quick ffprobe).
async fn frame_rate(st: &AppState, id: i64, path: &str) -> f64 {
    if let Some(f) = st.transcoder.fps.lock().unwrap().get(&id) {
        return *f;
    }
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=avg_frame_rate", "-of", "csv=p=0"])
        .arg(format!("file:{path}"))
        .output()
        .await;
    let fps = out
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_owned();
            let (n, d) = s.split_once('/')?;
            Some(n.parse::<f64>().ok()? / d.parse::<f64>().ok()?)
        })
        .filter(|f| f.is_finite() && *f > 1.0 && *f < 241.0)
        .unwrap_or(25.0);
    st.transcoder.fps.lock().unwrap().insert(id, fps);
    fps
}

async fn source(st: &AppState, id: i64) -> ApiResult<Source> {
    let (path, w, h, duration, streams): (String, Option<i64>, Option<i64>, Option<f64>, Option<String>) =
        sqlx::query_as("SELECT path, width, height, duration, streams FROM items WHERE id = ?").bind(id).fetch_one(&st.db).await?;
    let (Some(w), Some(h), Some(duration)) = (w, h, duration) else {
        return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "file has not been probed"));
    };
    let fps = frame_rate(st, id, &path).await;
    Ok(Source {
        path,
        width: w as u32,
        height: h as u32,
        duration,
        fps,
        streams: streams.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
    })
}

/// Encoder from the `transcode.hwaccel` setting: auto (NVENC, else VAAPI, when present) | nvenc | vaapi | none.
pub async fn hw_device(st: &AppState) -> Option<Hw> {
    let mode = crate::settings::get(&st.db, "transcode.hwaccel").await.unwrap_or_else(|| "auto".into());
    let dev = crate::settings::get(&st.db, "transcode.device").await.unwrap_or_else(|| "/dev/dri/renderD128".into());
    // The NVIDIA runtime exposes this node only when a GPU was handed to the container.
    let nvidia = FsPath::new("/dev/nvidiactl").exists();
    let vaapi = FsPath::new(&dev).exists();
    match mode.as_str() {
        "none" => None,
        "nvenc" if nvidia => Some(Hw::Nvenc),
        "vaapi" if vaapi => Some(Hw::Vaapi(dev)),
        "auto" if nvidia => Some(Hw::Nvenc),
        "auto" if vaapi => Some(Hw::Vaapi(dev)),
        _ => None,
    }
}

#[derive(Deserialize)]
pub struct MasterQ {
    #[serde(default = "no_audio")]
    audio: i64,
    t: Option<String>,
}

fn no_audio() -> i64 {
    -1
}

fn m3u8(body: String) -> Response {
    ([(header::CONTENT_TYPE, "application/vnd.apple.mpegurl"), (header::CACHE_CONTROL, "no-store")], body).into_response()
}

/// Master playlist: one H.264 rendition per ladder rung, all with the chosen audio track.
pub async fn master(State(st): State<AppState>, _u: User, Path(id): Path<i64>, Query(q): Query<MasterQ>) -> ApiResult<Response> {
    let src = source(&st, id).await?;
    let audio = if q.audio >= 0 { q.audio } else { first_audio(&src) };
    let t = q.t.unwrap_or_default();
    let mut out = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");
    for (p, kbps) in ladder(src.width, src.height) {
        let (w, h) = target_size(src.width, src.height, p);
        let bw = (kbps + if audio >= 0 { 160 } else { 0 }) * 1000;
        let codecs = if audio >= 0 { "avc1.640028,mp4a.40.2" } else { "avc1.640028" };
        out += &format!(
            "#EXT-X-STREAM-INF:BANDWIDTH={bw},AVERAGE-BANDWIDTH={},RESOLUTION={w}x{h},CODECS=\"{codecs}\",NAME=\"{p}p\"\n{p}/{audio}/index.m3u8?t={t}\n",
            bw * 4 / 5
        );
    }
    Ok(m3u8(out))
}

fn first_audio(src: &Source) -> i64 {
    let audio = src.streams.iter().filter(|s| s["type"] == "audio");
    audio.clone().find(|s| s["default"] == true).or(audio.clone().next()).and_then(|s| s["index"].as_i64()).unwrap_or(-1)
}

/// Variant playlist (`index.m3u8`) or one segment (`<n>.ts`).
pub async fn hls_file(
    State(st): State<AppState>,
    _u: User,
    Path((id, p, audio, file)): Path<(i64, u32, i64, String)>,
    Query(q): Query<MasterQ>,
) -> ApiResult<Response> {
    let src = source(&st, id).await?;
    if file == "index.m3u8" {
        let t = q.t.unwrap_or_default();
        let seg = src.seg_len();
        let count = (src.duration / seg).ceil().max(1.0) as u32;
        let mut out = format!(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:{}\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n",
            seg.ceil() as u32 + 1
        );
        for n in 0..count {
            let len = (src.duration - n as f64 * seg).min(seg);
            out += &format!("#EXTINF:{len:.3},\n{n}.ts?t={t}\n");
        }
        out += "#EXT-X-ENDLIST\n";
        return Ok(m3u8(out));
    }
    let n: u32 = file.strip_suffix(".ts").and_then(|s| s.parse().ok()).ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not found"))?;
    let hw = hw_device(&st).await;
    let path = st.transcoder.segment(id, &src, Variant { p, audio }, n, hw).await?;
    let body = tokio::fs::read(path).await.map_err(anyhow::Error::from)?;
    Ok(([(header::CONTENT_TYPE, "video/mp2t"), (header::CACHE_CONTROL, "no-store")], body).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_keep_aspect_and_orientation() {
        assert_eq!(target_size(1920, 1080, 720), (1280, 720));
        assert_eq!(target_size(720, 1280, 480), (480, 854));
        assert_eq!(target_size(854, 480, 1080), (854, 480)); // never upscale
    }

    #[test]
    fn ladder_starts_at_source() {
        let l: Vec<u32> = ladder(854, 480).iter().map(|r| r.0).collect();
        assert_eq!(l, vec![480, 360, 240, 144]);
        let l: Vec<u32> = ladder(3840, 2160).iter().map(|r| r.0).collect();
        assert_eq!(l[0], 1080);
    }
}
