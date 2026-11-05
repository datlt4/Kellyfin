//! Narration (thuyết minh). A worker in the film-sub compose (narrator/) pulls jobs from here. Per video, in four
//! stages: 1. subtitles in the spoken language (the video's own, else Whisper; "auto" detects the language),
//! 2. translation into English, Vietnamese and Chinese (Argos), 3. a voice reads the narration languages (VieNeu for
//! Vietnamese, edge-tts for Chinese), 4. mux, here on the server: the worker uploads subtitles and voices to
//! data/narration/<item>/ and takes the next video, while the server writes the subtitles next to the video
//! (`<stem>.<lang>.srt`, never over an existing file) and remuxes the video file itself with one more audio track per
//! voice — the voice over the ducked original sound, language-tagged — like the hand-made Energon episodes. The
//! video stream is copied, never re-encoded, and the new file is checked before it replaces the original.
use crate::{
    auth::Admin,
    error::{ApiError, ApiResult},
    pipeline, util, AppState,
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
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, io::AsyncWriteExt, process::Command};

/// Languages for subtitles and voices: (code used in file names and the API, ISO 639-2 tag for the track, label).
const LANGS: &[(&str, &str, &str)] = &[
    ("en", "eng", "English"),
    ("vi", "vie", "Vietnamese"),
    ("zh", "zho", "Chinese"),
    ("ja", "jpn", "Japanese"),
    ("ko", "kor", "Korean"),
    ("de", "deu", "German"),
    ("fr", "fra", "French"),
    ("es", "spa", "Spanish"),
    ("it", "ita", "Italian"),
    ("pt", "por", "Portuguese"),
    ("ru", "rus", "Russian"),
    ("th", "tha", "Thai"),
    ("id", "ind", "Indonesian"),
    ("ms", "msa", "Malay"),
];
/// A running job not heard from for this long is given to another worker.
const STALE_SECS: i64 = 900;
const TOKEN_KEY: &str = "narration.token";
const VOICE_VI_KEY: &str = "narration.voice_vi";
/// "1": the scanner lists lessons that only have subtitles, to be narrated into audio.
pub const LESSONS_KEY: &str = "narration.lessons";
/// "1" (default): one video at a time; "3": three at once (the old behaviour).
pub const JOBS_KEY: &str = "narration.jobs";
const VOICE_ZH_KEY: &str = "narration.voice_zh";
const DEFAULT_VOICE_VI: &str = "Ngọc Huyền";
const DEFAULT_VOICE_ZH: &str = "zh-CN-XiaoxiaoNeural";
/// The codes of LANGS.
const SUB_LANGS: &[&str] = &["en", "vi", "zh", "ja", "ko", "de", "fr", "es", "it", "pt", "ru", "th", "id", "ms"];
const AF: &str = "aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo";
const DUCK: &str = "sidechaincompress=threshold=0.01:ratio=5:attack=50:release=600";

/// Where a job's uploads wait for the mux.
pub fn dir(data: &FsPath, item: i64) -> PathBuf {
    data.join("narration").join(item.to_string())
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

/// A file name a worker may upload: `<lang>.srt`, `<lang>.voice.flac` (the voice alone) or `<lang>.podcast.mp3`
/// (the voice as the lesson's audio, for a lesson without a video).
fn upload_ok(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((l, "srt")) => SUB_LANGS.contains(&l),
        Some((l, "mp3")) => l.strip_suffix(".podcast").is_some_and(|l| LANGS.iter().any(|x| x.0 == l)),
        Some((l, "flac")) => l.strip_suffix(".voice").is_some_and(|l| LANGS.iter().any(|x| x.0 == l)),
        _ => false,
    }
}

/// Title of the tracks added here (kept by Matroska; MP4 keeps only the language tag).
fn track_title(name: &str) -> String {
    format!("Narration ({name})")
}

/// The original track split once per voice input; each copy ducks under its voice and gets it added, as [n<input>].
/// Voices are padded with silence first: sidechaincompress ends with its shorter input. A video without sound has
/// nothing to duck: its voices are only padded (endless, so the mux runs with -shortest and ends with the video).
fn mux_graph(audio: Option<i64>, inputs: &[usize]) -> String {
    let Some(audio) = audio else {
        return inputs.iter().map(|j| format!("[{j}:a:0]{AF},apad[n{j}]")).collect::<Vec<_>>().join(";");
    };
    let outs: String = inputs.iter().map(|j| format!("[og{j}]")).collect();
    let mut g = format!("[0:{audio}]{AF},asplit={}{outs}", inputs.len());
    for j in inputs {
        g += &format!(
            ";[{j}:a:0]{AF},apad,asplit=2[vs{j}][vm{j}];[og{j}][vs{j}]{DUCK}[d{j}];[d{j}][vm{j}]amix=inputs=2:duration=first:normalize=0[n{j}]"
        );
    }
    g
}

/// One mux at a time: the mini PC also transcodes, and each mux rewrites a whole video file.
fn mux_lock() -> &'static tokio::sync::Semaphore {
    static LOCK: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Semaphore::new(1))
}

/// Stage 4 of a job whose uploads are complete; then done (or failed, the video untouched).
async fn mux(st: AppState, id: i64) {
    let _turn = mux_lock().acquire().await;
    let (status, error) = match embed(&st, id).await {
        Ok(()) => ("done", None),
        Err(e) => ("failed", Some(format!("{e:#}"))),
    };
    tracing::info!("narration job {id} {status}{}", error.as_deref().map(|e| format!(": {e}")).unwrap_or_default());
    let _ = sqlx::query("UPDATE narration_jobs SET status = ?, step = NULL, progress = 1, error = ?, updated_at = ? WHERE id = ?")
        .bind(status)
        .bind(error)
        .bind(util::now())
        .bind(id)
        .execute(&st.db)
        .await;
}

async fn embed(st: &AppState, id: i64) -> anyhow::Result<()> {
    let (item, path, streams, duration): (i64, String, Option<String>, Option<f64>) = sqlx::query_as(
        "SELECT i.id, i.path, i.streams, i.duration FROM narration_jobs j JOIN items i ON i.id = j.item_id WHERE j.id = ?",
    )
    .bind(id)
    .fetch_one(&st.db)
    .await?;
    let streams: Vec<Value> = streams.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let d = dir(&st.cfg.data, item);
    if audio_only(&path) {
        return podcast(st, id, item, &path).await;
    }
    let video = FsPath::new(&path);
    let (Some(folder), Some(stem), Some(ext)) =
        (video.parent(), video.file_stem().and_then(|s| s.to_str()), video.extension().and_then(|e| e.to_str()))
    else {
        anyhow::bail!("unexpected file name {path}");
    };
    // New tracks: a voice from the worker to mix, or a track already mixed before 2.2.9 (<lang>.m4a).
    let mut voices: Vec<(&(&str, &str, &str), PathBuf, bool)> = Vec::new();
    for l in LANGS {
        let (voice, mixed) = (d.join(format!("{}.voice.flac", l.0)), d.join(format!("{}.m4a", l.0)));
        if voice.is_file() {
            voices.push((l, voice, false));
        } else if mixed.is_file() {
            voices.push((l, mixed, true));
        }
    }
    if !voices.is_empty() {
        sqlx::query("UPDATE narration_jobs SET step = ?, updated_at = ? WHERE id = ?")
            .bind("Muxing the narration into the video file (Kellyfin server)")
            .bind(util::now())
            .bind(id)
            .execute(&st.db)
            .await?;
        let a = source_audio(&streams);
        // Every original track stays, except an earlier narration in a language made now (a redo replaces it).
        let codes: Vec<&str> = voices.iter().map(|(l, ..)| l.0).collect();
        let keep: Vec<i64> = streams
            .iter()
            .filter(|s| s["type"] == "audio")
            .filter(|s| a.is_some_and(|a| s["index"] == a) || !s["lang"].as_str().and_then(lang_code).is_some_and(|c| codes.contains(&c)))
            .filter_map(|s| s["index"].as_i64())
            .collect();
        let isos: Vec<&str> = voices.iter().map(|(l, ..)| l.1).collect();
        // Hidden next to the video: same file system for the rename, and the scanner skips dot files.
        let tmp = folder.join(format!(".{stem}.narrating.{ext}"));
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin", "-y", "-i"]).arg(format!("file:{path}"));
        for (_, p, _) in &voices {
            cmd.arg("-i").arg(format!("file:{}", p.display()));
        }
        let mix: Vec<usize> = voices.iter().enumerate().filter(|(_, v)| !v.2).map(|(i, _)| i + 1).collect();
        if !mix.is_empty() {
            cmd.args(["-filter_complex", &mux_graph(a, &mix)]);
        }
        cmd.args(["-map", "0:v"]);
        for k in &keep {
            cmd.args(["-map", &format!("0:{k}")]);
        }
        for (i, (_, _, mixed)) in voices.iter().enumerate() {
            cmd.args(["-map", &if *mixed { format!("{}:a:0", i + 1) } else { format!("[n{}]", i + 1) }]);
        }
        cmd.args(["-map", "0:s?", "-c", "copy"]);
        // WebM takes Opus or Vorbis only; the other containers take AAC.
        let (codec, rate) = if ext.eq_ignore_ascii_case("webm") { ("libopus", "96k") } else { ("aac", "128k") };
        for (k, (l, _, mixed)) in voices.iter().enumerate() {
            let n = keep.len() + k;
            if !mixed {
                cmd.args([&format!("-c:a:{n}"), codec, &format!("-b:a:{n}"), rate]);
            }
            cmd.args([&format!("-metadata:s:a:{n}"), &format!("language={}", l.1)])
                .args([&format!("-metadata:s:a:{n}"), &format!("title={}", track_title(l.2))])
                .args([&format!("-disposition:a:{n}"), "0"]);
        }
        if matches!(ext.to_ascii_lowercase().as_str(), "mp4" | "m4v" | "mov") {
            cmd.args(["-movflags", "+faststart"]);
        }
        if a.is_none() {
            cmd.arg("-shortest");
        }
        let out = cmd.arg(&tmp).kill_on_drop(true).output().await?;
        if !out.status.success() {
            let _ = tokio::fs::remove_file(&tmp).await;
            anyhow::bail!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        if let Err(e) = verify(&tmp, keep.len() + voices.len(), &isos, duration).await {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(e);
        }
        tokio::fs::rename(&tmp, video).await?;
        // Stream indexes moved: drop the cached WebVTT of embedded subtitles, then probe the new file.
        let _ = tokio::fs::remove_dir_all(st.cfg.data.join("subs").join(item.to_string())).await;
        crate::scan::refresh_item(&st.db, item, &path).await?;
    }
    // Subtitles next to the video, where every player finds them; a file already there is replaced (a job always redoes them).
    if let Ok(mut rd) = tokio::fs::read_dir(&d).await {
        while let Some(e) = rd.next_entry().await? {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(lang) = name.strip_suffix(".srt").filter(|l| SUB_LANGS.contains(l)) {
                let target = folder.join(format!("{stem}.{lang}.srt"));
                // Copy beside it, then rename over it: replacing needs the folder to be writable, not the old file.
                let tmp = folder.join(format!(".{stem}.{lang}.srt.new"));
                tokio::fs::copy(e.path(), &tmp).await?;
                tokio::fs::rename(&tmp, &target).await?;
            }
        }
    }
    let _ = tokio::fs::remove_dir_all(&d).await;
    Ok(())
}

/// A lesson without a video: the first voice becomes `<name>.mp3` next to the subtitles (replacing an earlier one), the
/// translations `<name>.<lang>.srt`; the item then points at the mp3. Nothing is muxed.
async fn podcast(st: &AppState, id: i64, item: i64, path: &str) -> anyhow::Result<()> {
    let d = dir(&st.cfg.data, item);
    let src = FsPath::new(path);
    let (Some(folder), true) = (src.parent(), src.file_stem().is_some()) else { anyhow::bail!("unexpected file name {path}") };
    let stem = util::file_stem(path);
    let base = if util::is_srt(path) { util::split_lang(&stem).0.to_owned() } else { stem };
    let langs: String = sqlx::query_scalar("SELECT langs FROM narration_jobs WHERE id = ?").bind(id).fetch_one(&st.db).await?;
    let voice = langs.split(',').map(|l| d.join(format!("{l}.podcast.mp3"))).find(|p| p.is_file());
    let Some(voice) = voice else { anyhow::bail!("the worker uploaded no audio") };
    let target = folder.join(format!("{base}.mp3"));
    let tmp = folder.join(format!(".{base}.narrating.mp3"));
    tokio::fs::copy(&voice, &tmp).await?;
    tokio::fs::rename(&tmp, &target).await?;
    if let Ok(mut rd) = tokio::fs::read_dir(&d).await {
        while let Some(e) = rd.next_entry().await? {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(lang) = name.strip_suffix(".srt").filter(|l| SUB_LANGS.contains(l)) {
                let sub = folder.join(format!("{base}.{lang}.srt"));
                let tmp = folder.join(format!(".{base}.{lang}.srt.new"));
                tokio::fs::copy(e.path(), &tmp).await?;
                tokio::fs::rename(&tmp, &sub).await?;
            }
        }
    }
    let target = target.to_string_lossy().into_owned();
    if target != path {
        sqlx::query("UPDATE items SET path = ?, crop = NULL WHERE id = ?").bind(&target).bind(item).execute(&st.db).await?;
    }
    crate::scan::refresh_item(&st.db, item, &target).await?;
    let _ = tokio::fs::remove_dir_all(&d).await;
    Ok(())
}

/// The muxed file has a video, every expected audio track, the new languages, and the video's duration.
async fn verify(file: &FsPath, audio_tracks: usize, langs: &[&str], duration: Option<f64>) -> anyhow::Result<()> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type:stream_tags=language:format=duration", "-of", "json"])
        .arg(format!("file:{}", file.display()))
        .output()
        .await?;
    let v: Value = serde_json::from_slice(&out.stdout)?;
    let st = v["streams"].as_array().cloned().unwrap_or_default();
    let audio: Vec<&Value> = st.iter().filter(|s| s["codec_type"] == "audio").collect();
    if audio.len() != audio_tracks || !st.iter().any(|s| s["codec_type"] == "video") {
        anyhow::bail!("muxed file has {} audio tracks, expected {audio_tracks}", audio.len());
    }
    if let Some(l) = langs.iter().find(|l| !audio.iter().any(|s| s["tags"]["language"].as_str() == Some(**l))) {
        anyhow::bail!("muxed file lacks the {l} track");
    }
    let got: f64 = v["format"]["duration"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0.0);
    if duration.is_some_and(|d| (got - d).abs() > 2.0) {
        anyhow::bail!("muxed file is {got:.1} s long, the video {:.1} s", duration.unwrap_or(0.0));
    }
    Ok(())
}

/// At start-up: finish the muxes that were waiting or running, and embed narrations made before 2.2.9 (kept as
/// separate tracks in data/narration then).
pub async fn resume(st: AppState) {
    let jobs: Vec<(i64, i64, String)> = sqlx::query_as("SELECT id, item_id, status FROM narration_jobs WHERE status IN ('muxing', 'done') ORDER BY id")
        .fetch_all(&st.db)
        .await
        .unwrap_or_default();
    for (id, item, status) in jobs {
        let d = dir(&st.cfg.data, item);
        if status == "done" && !LANGS.iter().any(|l| d.join(format!("{}.m4a", l.0)).is_file()) {
            continue;
        }
        let _ = sqlx::query("UPDATE narration_jobs SET status = 'muxing', stage = 4, step = 'Waiting to mux (Kellyfin server)' WHERE id = ?")
            .bind(id)
            .execute(&st.db)
            .await;
        tokio::spawn(mux(st.clone(), id));
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
    edge_voices: Vec<String>,
}

#[derive(Default)]
pub struct Workers(Mutex<HashMap<String, Seen>>);

impl Workers {
    fn touch(&self, h: Hello) {
        self.0.lock().unwrap().insert(h.worker, Seen { at: Instant::now(), checks: h.checks, voices: h.voices, edge_voices: h.edge_voices });
    }

    /// A busy worker does not poll; its progress reports keep it listed (with its last service checks).
    fn alive(&self, name: &str) {
        let mut m = self.0.lock().unwrap();
        match m.get_mut(name) {
            Some(w) => w.at = Instant::now(),
            None => {
                m.insert(name.to_owned(), Seen { at: Instant::now(), checks: vec![], voices: vec![], edge_voices: vec![] });
            }
        }
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
                edge_voices: w.edge_voices.clone(),
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
    /// edge-tts voice names (e.g. "en-US-AriaNeural"), for the voice nodes.
    edge_voices: Vec<String>,
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
    /// "1": subtitles without a video or audio file become lessons (scan.rs).
    #[serde(default)]
    lessons: String,
    /// Videos worked on at once: "1" (default) or "3"; the worker clamps the server value to its own WORKER_JOBS.
    #[serde(default)]
    jobs: String,
}

async fn config(st: &AppState) -> Config {
    let get = |k| crate::settings::get(&st.db, k);
    Config {
        voice_vi: get(VOICE_VI_KEY).await.unwrap_or_else(|| DEFAULT_VOICE_VI.into()),
        voice_zh: get(VOICE_ZH_KEY).await.unwrap_or_else(|| DEFAULT_VOICE_ZH.into()),
        lessons: get(LESSONS_KEY).await.unwrap_or_default(),
        jobs: get(JOBS_KEY).await.unwrap_or_else(|| "1".into()),
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

/// Subtitles an item has, language code -> key: the external files beside the video and the embedded text tracks — the
/// same set a `subtitle` node can read (the claim hands this to the worker).
fn sub_map(item_path: &str, tracks: Vec<crate::stream::SubTrack>) -> HashMap<String, String> {
    let mut subs = HashMap::new();
    for t in tracks.into_iter().filter(|t| t.supported) {
        if let Some(code) = t.lang.as_deref().and_then(lang_code) {
            subs.entry(code.to_owned()).or_insert(t.key);
        }
    }
    if util::is_srt(item_path) {
        // The lesson is the subtitle file itself ("<name>.<lang>.srt").
        let stem = util::file_stem(item_path);
        if let Some(code) = util::split_lang(&stem).1.and_then(lang_code) {
            subs.insert(code.to_owned(), "self".to_owned());
        }
    }
    subs
}

/// A lesson narrated into an mp3 of its own: a subtitle file with no video yet, or an audio file made that way.
fn audio_only(path: &str) -> bool {
    util::is_srt(path) || util::is_audio(FsPath::new(path))
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
    #[serde(default)]
    edge_voices: Vec<String>,
    /// Jobs the worker is running now (it runs several): these are not cut off, so they are not put back.
    #[serde(default)]
    active: Vec<i64>,
    /// false: only a report, every slot of the worker is busy.
    #[serde(default = "yes")]
    take: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize)]
pub struct Claimed {
    job_id: i64,
    item_id: i64,
    title: String,
    duration: Option<f64>,
    /// What to run (pipeline.rs).
    pipeline: pipeline::Graph,
    /// Text subtitles the video already has, by language code -> key for `subtitle`.
    subs: HashMap<String, String>,
    /// A lesson with no video: the voice is made as an mp3 timeline of its own, line after line.
    audio_only: bool,
    #[serde(flatten)]
    config: Config,
}

/// Hands the worker the next queued job: the ones moved to the top first, then the oldest (204 when there is none).
pub async fn claim(State(st): State<AppState>, headers: HeaderMap, Json(h): Json<Hello>) -> ApiResult<Response> {
    worker_auth(&st, &headers).await?;
    let worker = h.worker.clone();
    let (active, take) = (serde_json::to_string(&h.active).map_err(anyhow::Error::from)?, h.take);
    st.narration_workers.touch(h);
    if !take {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let now = util::now();
    // A job still marked as the worker's own, that it does not list as running, was cut off (restart): back it goes.
    sqlx::query(
        "UPDATE narration_jobs SET status = 'queued', step = NULL, progress = 0, updated_at = ?2
         WHERE status = 'running' AND worker = ?1 AND id NOT IN (SELECT value FROM json_each(?3))",
    )
    .bind(&worker)
    .bind(now)
    .bind(&active)
    .execute(&st.db)
    .await?;
    let Some((job_id, item_id, source, langs, graph)): Option<(i64, i64, String, String, Option<String>)> = sqlx::query_as(
        "UPDATE narration_jobs SET status = 'running', worker = ?1, stage = 0, step = 'starting', progress = 0, error = NULL,
           updated_at = ?2
         WHERE id = (SELECT id FROM narration_jobs
                     WHERE status = 'queued' OR (status = 'running' AND updated_at < ?3 AND COALESCE(worker, '') != ?1)
                     ORDER BY priority DESC, id LIMIT 1)
         RETURNING id, item_id, source, langs, pipeline",
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
    let item_path = path.clone();
    let streams: Value = streams.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let tracks = tokio::task::spawn_blocking(move || crate::stream::subtitle_tracks(&path, &streams))
        .await
        .map_err(anyhow::Error::from)?;
    let subs = sub_map(&item_path, tracks);
    let audio_only = audio_only(&item_path);
    tracing::info!("narration job {job_id} (item {item_id}) -> {worker}");
    let pipeline = match graph.and_then(|g| serde_json::from_str(&g).ok()) {
        Some(g) => g,
        None => legacy_graph(&source, &langs),
    };
    Ok(Json(Claimed { job_id, item_id, title, duration, pipeline, subs, audio_only, config: config(&st).await }).into_response())
}

/// Jobs queued before pipelines (2.3.0): the Whisper preset with their spoken language and voices.
fn legacy_graph(source: &str, langs: &str) -> pipeline::Graph {
    let (_, _, mut g) = pipeline::builtins().remove(0);
    let keep: Vec<&str> = langs.split(',').collect();
    for n in g.nodes.iter_mut().filter(|n| n.kind == "whisper") {
        n.lang = source.into();
    }
    let drop: Vec<String> = [("vvi", "vi"), ("vzh", "zh")].iter().filter(|(_, l)| !keep.contains(l)).map(|(id, _)| (*id).into()).collect();
    g.nodes.retain(|n| !drop.contains(&n.id));
    g.edges.retain(|(a, b)| !drop.contains(a) && !drop.contains(b));
    g
}

/// The whole video of a claimed job, for OCR of burned-in subtitles (range requests supported).
pub async fn video(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<i64>, req: axum::extract::Request) -> ApiResult<Response> {
    worker_auth(&st, &headers).await?;
    let (_, path, _) = job_item(&st, id).await?;
    let res = tower::ServiceExt::oneshot(tower_http::services::ServeFile::new(path), req).await.map_err(|e| anyhow::anyhow!(e))?;
    Ok(res.map(Body::new).into_response())
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
    let (item, path, _) = job_item(&st, id).await?;
    if key == "self" && util::is_srt(&path) {
        let text = String::from_utf8_lossy(&tokio::fs::read(&path).await.map_err(anyhow::Error::from)?).into_owned();
        return Ok(([(axum::http::header::CONTENT_TYPE, "application/x-subrip; charset=utf-8")], text).into_response());
    }
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
    let worker: Option<Option<String>> = sqlx::query_scalar(
        "UPDATE narration_jobs SET stage = ?, step = ?, progress = ?, updated_at = ? WHERE id = ? AND status = 'running' RETURNING worker",
    )
    .bind(p.stage.clamp(0, 4))
    .bind(p.step.chars().take(200).collect::<String>())
    .bind(p.progress.clamp(0.0, 1.0))
    .bind(util::now())
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    match worker {
        Some(Some(w)) => st.narration_workers.alive(&w),
        // Deleted or paused meanwhile: the worker stops this job.
        None => return Ok(StatusCode::GONE),
        _ => {}
    }
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

/// The worker has uploaded everything: the server muxes (stage 4) and the worker takes the next video.
pub async fn done(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    worker_auth(&st, &headers).await?;
    let n = sqlx::query(
        "UPDATE narration_jobs SET status = 'muxing', stage = 4, step = 'Waiting to mux (Kellyfin server)', progress = 0.9,
           error = NULL, updated_at = ? WHERE id = ? AND status = 'running'",
    )
    .bind(util::now())
    .bind(id)
    .execute(&st.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Ok(StatusCode::GONE); // deleted or paused while the worker finished: nothing to mux
    }
    tokio::spawn(mux(st, id));
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Admin ----------

#[derive(Serialize, sqlx::FromRow)]
pub struct JobRow {
    id: i64,
    item_id: i64,
    title: String,
    library: String,
    preset: Option<String>,
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
        "SELECT j.id, j.item_id, i.title, l.name AS library, j.preset, j.source, j.langs, j.status, j.stage, j.step, j.progress, j.error,
           j.worker, j.updated_at
         FROM narration_jobs j JOIN items i ON i.id = j.item_id JOIN libraries l ON l.id = i.library_id
         ORDER BY CASE j.status WHEN 'running' THEN 0 WHEN 'muxing' THEN 0 WHEN 'queued' THEN 1 WHEN 'paused' THEN 2 WHEN 'failed' THEN 3 ELSE 4 END,
           CASE WHEN j.status IN ('queued', 'paused') THEN -j.priority ELSE 0 END,
           CASE WHEN j.status IN ('queued', 'paused') THEN j.id ELSE -j.id END
         LIMIT 500",
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(Overview { config: config(&st).await, token: token(&st).await?, workers: st.narration_workers.online(), jobs }))
}

pub async fn save_config(State(st): State<AppState>, _a: Admin, Json(c): Json<Config>) -> ApiResult<StatusCode> {
    for (k, v) in [
        (VOICE_VI_KEY, c.voice_vi.trim()),
        (VOICE_ZH_KEY, c.voice_zh.trim()),
        (LESSONS_KEY, if c.lessons == "1" { "1" } else { "0" }),
        (JOBS_KEY, if c.jobs == "3" { "3" } else { "1" }),
    ] {
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
    /// A built-in preset id ("whisper", "ocr") or a saved one's number; default "whisper".
    #[serde(default)]
    preset: Option<String>,
}

/// A preset by id: built-in ones by name, saved ones by number. (name, graph)
async fn preset(st: &AppState, id: &str) -> ApiResult<(String, pipeline::Graph)> {
    if let Some((_, name, g)) = pipeline::builtins().into_iter().find(|(b, ..)| *b == id) {
        return Ok((name.into(), g));
    }
    let n: i64 = id.parse().map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "unknown pipeline"))?;
    let (name, graph): (String, String) = sqlx::query_as("SELECT name, graph FROM narration_presets WHERE id = ?")
        .bind(n)
        .fetch_optional(&st.db)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown pipeline"))?;
    Ok((name, serde_json::from_str(&graph).map_err(anyhow::Error::from)?))
}

fn pipeline_has_media_source(g: &pipeline::Graph) -> bool {
    g.nodes.iter().any(|n| matches!(n.kind.as_str(), "whisper" | "ocr"))
}

#[derive(Serialize)]
pub struct QueueResult {
    queued: u64,
    /// Videos (or lessons) the pipeline has no input subtitles for; they were skipped, not queued.
    skipped: u64,
}

/// Queues videos (or a whole library) with a pipeline. A failed or finished job is queued again and redone from the
/// start (subtitles, translation, voices); one that is waiting, running or paused is left alone. A pipeline whose only
/// source is a subtitle file skips (and counts in `skipped`) the items that have none of the subtitle languages it asks
/// for — found the same way the claim finds them for the worker (external files beside the video, embedded text tracks).
pub async fn queue(State(st): State<AppState>, _a: Admin, Json(q): Json<Queue>) -> ApiResult<Json<QueueResult>> {
    let (name, graph) = preset(&st, q.preset.as_deref().unwrap_or("whisper")).await?;
    let summary = pipeline::validate(&graph, SUB_LANGS).map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))?;
    let graph_value = graph.clone();
    let graph = serde_json::to_string(&graph).map_err(anyhow::Error::from)?;
    let mut ids = q.item_ids;
    if let Some(lib) = q.library_id {
        ids.extend(sqlx::query_scalar::<_, i64>("SELECT id FROM items WHERE library_id = ? ORDER BY sort_title, id").bind(lib).fetch_all(&st.db).await?);
    }
    let now = util::now();
    let mut added = 0;
    // A lesson without a video has nothing to transcribe or read off the picture: only subtitle sources fit.
    let only_subs = !pipeline_has_media_source(&graph_value);
    let input_langs: Vec<String> = graph_value.nodes.iter().filter(|n| n.kind == "subtitle").map(|n| n.lang.clone()).collect();
    let must_have_input = !input_langs.is_empty() && only_subs;
    let mut skipped = 0u64;
    for id in ids {
        let row: Option<(String, Option<String>)> = sqlx::query_as("SELECT path, streams FROM items WHERE id = ?").bind(id).fetch_optional(&st.db).await?;
        let Some((path, streams)) = row else { continue };
        if audio_only(&path) && !only_subs {
            continue;
        }
        if must_have_input {
            let streams: Value = streams.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            let p = path.clone();
            let tracks = tokio::task::spawn_blocking(move || crate::stream::subtitle_tracks(&p, &streams)).await.unwrap_or_default();
            let subs = sub_map(&path, tracks);
            let missing = input_langs.iter().any(|l| match lang_code(l) {
                Some(c) => !subs.contains_key(c),
                None => true,
            });
            if missing {
                skipped += 1;
                continue;
            }
        }
        added += sqlx::query(
            "INSERT INTO narration_jobs (item_id, source, langs, preset, pipeline, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT (item_id) DO UPDATE SET status = 'queued', source = excluded.source, langs = excluded.langs,
               preset = excluded.preset, pipeline = excluded.pipeline, stage = 0, step = NULL, progress = 0, error = NULL,
               priority = 0, updated_at = excluded.updated_at
             WHERE narration_jobs.status IN ('failed', 'done')",
        )
        .bind(id)
        .bind(&summary.source)
        .bind(summary.voices.join(","))
        .bind(&name)
        .bind(&graph)
        .bind(now)
        .execute(&st.db)
        .await?
        .rows_affected();
    }
    Ok(Json(QueueResult { queued: added, skipped }))
}

#[derive(Deserialize)]
pub struct InputCheck {
    library_id: i64,
    /// Same preset argument as `queue`: a built-in id or a saved preset's number.
    #[serde(default)]
    preset: Option<String>,
}

#[derive(Serialize)]
pub struct InputCheckResult {
    /// Videos of the library with none of the pipeline's input subtitles (relative to the library's folders); a queue would skip them.
    videos_without_input: Vec<String>,
    /// Subtitle files with no video or audio file beside them (relative to the library's folders): a lesson outputs audio only.
    subtitles_without_video: Vec<String>,
}

/// File name relative to the longest root that holds it (the library's folder), e.g. "03 - Sketching/003 x.mp4".
fn rel(roots: &[String], path: &str) -> String {
    use std::path::Path;
    roots
        .iter()
        .filter_map(|r| Path::new(path).strip_prefix(r).ok().map(|p| p.to_path_buf()))
        .min_by_key(|p| p.components().count())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

/// What queueing with this pipeline would do: which videos lack its input subtitles (skipped), and which subtitle files
/// have no video or audio file (a lesson outputs audio only). Both lists are relative to the library's folders.
pub async fn input_check(State(st): State<AppState>, _a: Admin, Json(q): Json<InputCheck>) -> ApiResult<Json<InputCheckResult>> {
    let (_name, graph) = preset(&st, q.preset.as_deref().unwrap_or("whisper")).await?;
    let input_langs: Vec<String> = graph.nodes.iter().filter(|n| n.kind == "subtitle").map(|n| n.lang.clone()).collect();
    let only_subs = !pipeline_has_media_source(&graph);
    let roots: Vec<String> = sqlx::query_scalar("SELECT path FROM library_paths WHERE library_id = ?").bind(q.library_id).fetch_all(&st.db).await?;
    let mut no_input: Vec<String> = Vec::new();
    if !input_langs.is_empty() && only_subs {
        let rows: Vec<(String, Option<String>)> = sqlx::query_as("SELECT path, streams FROM items WHERE library_id = ?").bind(q.library_id).fetch_all(&st.db).await?;
        for (path, streams) in rows {
            let streams: Value = streams.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            let p = path.clone();
            let tracks = tokio::task::spawn_blocking(move || crate::stream::subtitle_tracks(&p, &streams)).await.unwrap_or_default();
            let subs = sub_map(&path, tracks);
            let missing = input_langs.iter().any(|l| match lang_code(l) {
                Some(c) => !subs.contains_key(c),
                None => true,
            });
            if missing {
                no_input.push(rel(&roots, &path));
            }
        }
        no_input.sort();
        no_input.truncate(100);
    }
    let roots2 = roots.clone();
    let orphans = tokio::task::spawn_blocking(move || crate::scan::walk(&roots2, true).into_keys().filter(|p| util::is_srt(p)).collect::<Vec<_>>())
        .await
        .unwrap_or_default();
    let mut orphans: Vec<String> = orphans.into_iter().map(|p| rel(&roots, &p)).collect();
    orphans.sort();
    orphans.truncate(100);
    Ok(Json(InputCheckResult { videos_without_input: no_input, subtitles_without_video: orphans }))
}

// ---------- Presets ----------

#[derive(Serialize)]
pub struct PresetRow {
    id: String,
    name: String,
    builtin: bool,
    graph: pipeline::Graph,
}

pub async fn presets(State(st): State<AppState>, _a: Admin) -> ApiResult<Json<Vec<PresetRow>>> {
    let mut out: Vec<PresetRow> =
        pipeline::builtins().into_iter().map(|(id, name, graph)| PresetRow { id: id.into(), name: name.into(), builtin: true, graph }).collect();
    let saved: Vec<(i64, String, String)> = sqlx::query_as("SELECT id, name, graph FROM narration_presets ORDER BY name").fetch_all(&st.db).await?;
    for (id, name, graph) in saved {
        if let Ok(graph) = serde_json::from_str(&graph) {
            out.push(PresetRow { id: id.to_string(), name, builtin: false, graph });
        }
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct PresetBody {
    name: String,
    graph: pipeline::Graph,
}

fn checked(b: &PresetBody) -> ApiResult<(String, String)> {
    let name = b.name.trim();
    if name.is_empty() || pipeline::builtins().iter().any(|(_, n, _)| *n == name) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "give the pipeline its own name"));
    }
    pipeline::validate(&b.graph, SUB_LANGS).map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))?;
    Ok((name.to_owned(), serde_json::to_string(&b.graph).map_err(anyhow::Error::from)?))
}

pub async fn create_preset(State(st): State<AppState>, _a: Admin, Json(b): Json<PresetBody>) -> ApiResult<Json<String>> {
    let (name, graph) = checked(&b)?;
    let id: i64 = sqlx::query_scalar("INSERT INTO narration_presets (name, graph, updated_at) VALUES (?, ?, ?) RETURNING id")
        .bind(name)
        .bind(graph)
        .bind(util::now())
        .fetch_one(&st.db)
        .await
        .map_err(|_| ApiError::new(StatusCode::CONFLICT, "a pipeline with that name exists"))?;
    Ok(Json(id.to_string()))
}

pub async fn update_preset(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>, Json(b): Json<PresetBody>) -> ApiResult<StatusCode> {
    let (name, graph) = checked(&b)?;
    sqlx::query("UPDATE narration_presets SET name = ?, graph = ?, updated_at = ? WHERE id = ?")
        .bind(name)
        .bind(graph)
        .bind(util::now())
        .bind(id)
        .execute(&st.db)
        .await
        .map_err(|_| ApiError::new(StatusCode::CONFLICT, "a pipeline with that name exists"))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_preset(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    sqlx::query("DELETE FROM narration_presets WHERE id = ?").bind(id).execute(&st.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Clear {
    /// "done" or "failed".
    status: String,
}

/// Tidies the queue: removes the finished, the failed or the paused jobs; what was muxed into the videos stays.
pub async fn clear(State(st): State<AppState>, _a: Admin, Json(c): Json<Clear>) -> ApiResult<Json<u64>> {
    if !matches!(c.status.as_str(), "done" | "failed" | "paused") {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "clear done, failed or paused jobs"));
    }
    let items: Vec<i64> =
        sqlx::query_scalar("DELETE FROM narration_jobs WHERE status = ? RETURNING item_id").bind(&c.status).fetch_all(&st.db).await?;
    for item in &items {
        let _ = tokio::fs::remove_dir_all(dir(&st.cfg.data, *item)).await; // a failed job's leftover uploads
    }
    Ok(Json(items.len() as u64))
}

#[derive(Deserialize)]
pub struct Which {
    /// Empty: every waiting (or every paused) job.
    #[serde(default)]
    ids: Vec<i64>,
}

/// Pauses waiting jobs (all, or the listed ones). A listed running job stops where it is (the worker is told at its
/// next report) and starts over when resumed.
pub async fn pause(State(st): State<AppState>, _a: Admin, Json(w): Json<Which>) -> ApiResult<Json<u64>> {
    let ids = serde_json::to_string(&w.ids).map_err(anyhow::Error::from)?;
    let n = sqlx::query(
        "UPDATE narration_jobs SET status = 'paused', stage = 0, step = NULL, progress = 0, updated_at = ?1
         WHERE (status = 'queued' AND (json_array_length(?2) = 0 OR id IN (SELECT value FROM json_each(?2))))
            OR (status = 'running' AND id IN (SELECT value FROM json_each(?2)))",
    )
    .bind(util::now())
    .bind(ids)
    .execute(&st.db)
    .await?
    .rows_affected();
    Ok(Json(n))
}

pub async fn resume_jobs(State(st): State<AppState>, _a: Admin, Json(w): Json<Which>) -> ApiResult<Json<u64>> {
    let ids = serde_json::to_string(&w.ids).map_err(anyhow::Error::from)?;
    let n = sqlx::query(
        "UPDATE narration_jobs SET status = 'queued', updated_at = ?1
         WHERE status = 'paused' AND (json_array_length(?2) = 0 OR id IN (SELECT value FROM json_each(?2)))",
    )
    .bind(util::now())
    .bind(ids)
    .execute(&st.db)
    .await?
    .rows_affected();
    Ok(Json(n))
}

/// Moves a waiting (or paused) job in front of every other: the worker claims it next.
pub async fn top(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    sqlx::query(
        "UPDATE narration_jobs SET status = 'queued', updated_at = ?1, priority = (SELECT COALESCE(MAX(priority), 0) + 1 FROM narration_jobs)
         WHERE id = ?2 AND status IN ('queued', 'paused')",
    )
    .bind(util::now())
    .bind(id)
    .execute(&st.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A failed job runs again with the pipeline it was queued with.
pub async fn retry(State(st): State<AppState>, _a: Admin, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    sqlx::query(
        "UPDATE narration_jobs SET status = 'queued', stage = 0, step = NULL, progress = 0, error = NULL, updated_at = ?
         WHERE id = ? AND status = 'failed'",
    )
    .bind(util::now())
    .bind(id)
    .execute(&st.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes the job and its waiting uploads; tracks and subtitles already muxed into the folder stay.
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
    fn languages_uploads_and_graph() {
        assert_eq!(lang_code("eng"), Some("en"));
        assert_eq!(lang_code("Vietnamese"), Some("vi"));
        assert_eq!(lang_code("zh-Hans"), Some("zh"));
        assert_eq!(lang_code("kor"), Some("ko"));
        assert_eq!(lang_code("xx"), None);
        assert!(upload_ok("ko.srt") && upload_ok("vi.voice.flac"));
        assert!(upload_ok("ko.voice.flac"));
        assert!(!upload_ok("vi.m4a") && !upload_ok("xx.voice.flac") && !upload_ok("../x.srt") && !upload_ok("vi.wav"));
        let g = mux_graph(Some(1), &[1, 2]);
        assert!(g.starts_with("[0:1]aformat") && g.contains("asplit=2[og1][og2]") && g.ends_with("[n2]"));
        let g = mux_graph(None, &[1, 2]);
        assert!(g.starts_with("[1:a:0]aformat") && g.ends_with("apad[n2]") && !g.contains("[0:"));
        assert_eq!(track_title("Chinese"), "Narration (Chinese)");
    }

    #[test]
    fn sub_map_finds_external_and_lesson_self() {
        let d = std::env::temp_dir().join(format!("kellyfin-submap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for f in ["001 A.mp4", "001 A.en.srt", "002 B.en.srt", "003 C.mp4"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        let null = serde_json::Value::Null;
        let path = |n| d.join(n).to_string_lossy().into_owned();
        // A video with the .en.srt beside it.
        let t = crate::stream::subtitle_tracks(&path("001 A.mp4"), &null);
        assert_eq!(sub_map(&path("001 A.mp4"), t).get("en").map(String::as_str), Some("x0"));
        // A lesson without a video reads its own file as "self".
        let t = crate::stream::subtitle_tracks(&path("002 B.en.srt"), &null);
        assert_eq!(sub_map(&path("002 B.en.srt"), t).get("en").map(String::as_str), Some("self"));
        // A video without subtitles has nothing.
        let t = crate::stream::subtitle_tracks(&path("003 C.mp4"), &null);
        assert!(sub_map(&path("003 C.mp4"), t).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
