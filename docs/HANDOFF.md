# Kellyfin — handoff

State on 2026-11-08, version **2.19.5** (2.0.0: in-app updates, AI upscaling, Jellyfin retired; 2.0.1: iOS settings fixes; 2.0.2: iOS proxy restart after suspension, pull to refresh; 2.0.3: pull gesture no longer cancelled by first-touch jitter; 2.0.4: player swipes removed, no page zoom, "10" in skip icons; 2.0.5: rescans refresh thumbnails of replaced files, scan progress in Libraries, no iOS landscape text inflation; 2.0.6: black bars detected by the scan and cropped in the player and thumbnails; 2.0.7: crop applied as soon as the video size is known, round icon buttons on phones; 2.0.8: no endless spinner at the end of transcoded videos (its release run was cancelled; shipped in 2.0.9); 2.0.9: previous/next around the clock, centre play/±10 s on every screen; 2.0.10: several transcodes per video, hand-drawn volume slider, desktop icon with a margin; 2.0.11: the Linux app plays HEVC itself instead of a transcode; 2.0.12: Linux fullscreen without the WebKitGTK crash, hand-drawn volume bar, reload in the desktop apps; 2.1.0: start-up timeout for unreachable servers, exact seek bar, Alt/Option+P, mouse-leave fade; 2.1.1: sound keeps playing in the background on phones; 2.1.2: auto-play next switch, watched videos resume and show in Continue; 2.1.3: no spinner at the end when the audio track is shorter than the video; 2.2.0: narration (thuyết minh) pipeline with a film-sub worker; 2.2.1: narration settings fixes; 2.2.2: the narration worker reports the services, no film-sub URL in Kellyfin; 2.2.3: edge-tts check no longer leaks a connection; 2.2.4: Narration button on library pages; 2.2.5: sturdier narration worker; 2.2.6: the narration mux runs on the Kellyfin server (broken start-up, fixed in 2.2.7); 2.2.7: migration 0006 restored, CI guards released migrations; 2.2.8: narration skips lines with nothing to read; 2.2.9: narration muxed into the video files, subtitles beside them; 2.2.10: VieNeu voice picker; 2.2.11: Whisper repeats merged; 2.3.0: narration pipelines as node graphs, OCR of burned-in subtitles; 2.4.0: filterable drop-downs, full-screen pipeline canvas, multi-language OCR, edge-tts voice lists, lazy pipelines; 2.4.1: edge-tts truncated lines made again, OCR language tags, queue clearing; 2.4.2: OCR ignores text behind the picture; 2.4.3: narration into WebM and into videos without sound, filter in the folder picker; 2.4.4: film-sub on the CPU; 2.5.0: several videos at once in the worker, queue pause / top; 2.5.1: queueing no longer needs VieNeu, edge-tts-only pipelines; 2.5.2: visible errors, queue shows reused subtitles; 2.5.3: edge-tts retries and one connection limit; 2.5.4: a deleted job stops within seconds; 2.6.0: audio lessons — subtitles without a video become an mp3, played on a black screen; 2.7.0: folder tree view, previous/next in tree order; 2.8.0: the tree view nests folders to any depth, VS Code style, and the videos are rows or grid cards; 2.8.1: the tree list is gone — the tree is always grid cards — and the folder rows are larger with wider spacing; 2.9.0: the narration queue takes a "Videos at once" setting (sequential by default, one video at a time; the old three-at-once is a Parallel option), and the queue gets a "Clear paused" bulk button; 2.10.0: a "subtitle input" pipeline skips (and counts) the items that have none of the subtitle languages it asks for, when queueing a library or videos; 2.11.0: the Narration tab shows a note under the pipeline picker with the videos in the chosen folder that have no input subtitles (and that are subtitle-only lessons: audio-only subs, with a pointer to the lessons toggle); 2.12.0: the TTS stage cannot hang any more — 20 s timeout per edge-tts line, and Google Translate TTS as fallback (per line, or the whole job when the edge-tts probe fails); 2.13.0: OpenRouter OCR — a new engine choice on the OCR node (free vision models, list fetched live from OpenRouter, embeddings excluded, primary + fallback per user in Settings → Profile alongside the user's own API key, AES-256-GCM encrypted with KELLYFIN_OPENROUTER_SECRET in .env, jobs refuse to start when a model has stopped being free)). 2.19.0: no `content-visibility: auto` in the scrolling views — on macOS (WKWebView) a card skipped off-screen was sometimes painted at a stale offset and its intrinsic width, leaving a big blank row inside a folder and dropping that card far below it in the library tree (owner's FreeCAD library: the 7th video of "02-Getting started…" sat ~1000 px down in the middle of folders 03/04 and was 1 column wide, with an empty card row above); `.card`, `.row-item` and the Home `.row` shelves no longer skip their paint (lists are paginated at 120, the tree shows one library). 2.18.0: OpenRouter OCR jobs no longer burn the key's free-model daily allowance into pointless retries — the worker stops holding batches on 429 (both models tried once, then the job dies with an error naming the shared 1000 free requests/day cap), and the claim fails a job immediately (clients never see it start) when the key's `free_model_daily_requests.remaining` is 0. 2.17.0: pipelines whose OCR node uses the OpenRouter engine no longer demand the EasyOCR service in "Check server and queue" — the worker reports an OpenRouter health probe (GET /api/v1/models) and the preflight asks for it; the film-sub stack no longer runs EasyOCR at all. 2.16.0: the OpenRouter OCR profile panel loaded its model list never — the models resource resolved once (before the key was saved) and never re-ran, so the dropdown showed "No models loaded yet" forever; the resource now depends on `key_set` (source + fetcher), refetches after every save, and shows loading/error instead of a misleading placeholder. 2.15.0: empty subtitle files are not input anywhere — a 0-byte .en.srt that used to count as "this lesson has English input" is skipped at queue time (the 5 FreeCAD lessons failed with "Argos returned the en text unchanged for vi", which was really zero cues hitting the all-identical check); the worker also says what actually happened ("the en subtitles are empty — no lines to translate")). 2.14.0: videos without sound can be narrated again — the mux no longer overshoots the video (the voice is padded with endless silence and `-shortest` does not cut a stream-copied video; the output is clamped with `-t <video duration>`)). Since 2.1.2 there is only `main` (the `dev` branch was deleted at the owner's request): every change is committed to `main`. 1.0.0 was the first release: `dev` was fast-forwarded into `main`, which runs
`release.yml` (branch `release/v1.0.0`, tag `v1.0.0`, GitHub release with desktop and mobile builds).
This file is updated with every change (owner's rule): keep it that way.
Read this before touching the code: it explains how the pieces fit, why they are built the way they are
(most choices came from measurements on the real library), how to build and ship without GitHub Actions,
and what is still open or unverified.

No secrets are in this file or the repo (the repo is public). Where a secret is needed, its location is given.

---

## 1. What Kellyfin is

A self-hosted replacement for Jellyfin, written from scratch:

- **Server**: Rust (axum + SQLite + ffmpeg), one Docker image, runs on a mini PC with an AMD iGPU.
- **Client**: SolidJS + TypeScript web app (≈25 KB gzip JS, so it stays fast on an old Samsung A8 Star),
  served by the server for browsers and bundled by **Tauri 2** for desktop and mobile apps.
- Goals the owner set: YouTube-smooth playback on LAN and over WireGuard, fast thumbnails, resolution change
  without pre-encoding, subtitle/audio track choice, suggestions by title, cast, multi-platform apps, and later
  an experimental AI upscaler (see §8).

### Phase status

| Phase | Scope | State |
|---|---|---|
| P0 | Repo skeleton, Docker, CI | done |
| P1 | Library scan, ffprobe, thumbnails, Jellyfin migration | done, deployed with real data |
| P2 | Player: direct play, remux, subtitles, audio tracks, progress, PiP | done |
| P3 | On-demand HLS transcoding (VAAPI / NVENC / x264), quality menu | done |
| P4 | Settings, dashboard, users, libraries, search, title-based suggestions | done |
| P5 | Multi-platform releases, mobile apps, IPA re-sign jobs | **v1.0.0 released** (https://github.com/datlt4/Kellyfin/releases/tag/v1.0.0); re-sign jobs and device installs still to do (§6) |
| P6 | Chromecast + AirPlay | code done, **not tested with a real device** |
| P7 | AI upscale (GPU server) | **pipeline done** with a stub model; waiting for trained models (§8) |

---

## 2. Machines and access

| Machine | Role | Access |
|---|---|---|
| **koi-pc** (Ubuntu 22.04, i5-12400F, RTX 3060) | development box, Docker builds, Linux/Windows desktop builds | local; no Rust toolchain installed — Rust runs in Docker (see §5) |
| **mini PC** `koi-mini` (Ubuntu 22.04, Ryzen 5 3400GE, Vega 11 iGPU, 8 TB DAS) | runs Kellyfin and the old Jellyfin | `ssh mini-wg`; WireGuard IP `24.8.0.2`, LAN `192.168.1.15` |
| **mabu** (MacBook, Intel, macOS 13.7, Xcode 15.2) | iOS/Android/macOS builds, IPA re-sign dashboard | `ssh mabu-wg` (password auth; ask the owner) |

On the mini PC:

- Compose project: `/home/mini/Software/kellyfin/` (`docker-compose.yml` + `.env` with `KELLYFIN_TAG=latest`).
- Data (SQLite DB, images, transcode scratch): `/mnt/das_8tb/kellyfin_volume/data`.
- Jellyfin export used for the migration: `/mnt/das_8tb/kellyfin_volume/jellyfin-export/`.
- Kellyfin listens on **:8024** (→ container :8080). Jellyfin was retired on 2026-10-20: container, image,
  `config/` and `cache/` removed; `/home/mini/Software/jellyfin/docker-compose.yml` kept; the videos and subtitles in
  `jellyfin_volume/{media,movies,hanzii}` and `metube_volume` are untouched (Kellyfin uses them). The Jellyfin
  export in `kellyfin_volume/jellyfin-export/` is the only copy of its metadata.
- Media is mounted at the same paths Jellyfin used: `/media`, `/movies`, `/hanzii`, `/metube_volume` — read-only until
  2.2.9, writable since then for narration (the only code that writes there: muxed audio tracks, `.srt` beside videos).
- Public URL `https://kellyfin.ft-tech.io.vn` (reverse proxy; may sit behind Pangolin with header auth).
- GPU: `/dev/dri` + group `110` (render) for VAAPI.

Secrets (never commit them):

| Secret | Location |
|---|---|
| Kellyfin admin password | set by the owner (`docker exec kellyfin kellyfin passwd root …` to reset) |
| Android release keystore + password | `~/.config/kellyfin/android-release.{jks,pass}` on koi-pc **and** mabu (back it up: losing it means users must uninstall to update) |
| GitHub token (contents: write on datlt4/Kellyfin) | `~/.config/kellyfin/github-token` on koi-pc |
| Docker Hub (`hienanh`) | `docker login` on koi-pc; repo secrets `DOCKERHUB_USERNAME/TOKEN` for CI |
| Tauri updater signing key + password + public key | `~/.config/kellyfin/tauri-updater.{key,pass,key.pub}` on koi-pc; repo secrets `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)`; losing it strands installed desktop apps on their version |
| Upscaler worker token | Settings → AI upscale (setting `upscaler.token`) |

---

## 3. Repository map

```
server/                 Rust server (binary `kellyfin`)
  migrations/           SQLite schema (sqlx migrate!, embedded)
  src/main.rs           CLI: serve | scan | import | export | jellyfin-export | passwd
  src/api.rs            router, cards, home/library/item endpoints
  src/auth.rs           argon2 passwords, bearer sessions (also ?t= for media elements)
  src/scan.rs           walk folders -> ffprobe -> frame-grab thumbnails
  src/images.rs         WebP variants s/m/l (400/800/1280 px) + average color, via ffmpeg
  src/stream.rs         direct file (range), copy-remux to fMP4, keyframe lookup, subtitles -> WebVTT, progress
  src/transcode.rs      on-demand HLS: up to 3 ffmpeg jobs per item, throttling, VAAPI/NVENC/x264
  src/recommend.rs      title tokens, related libraries/videos, next season, search
  src/admin.rs          dashboard, users, libraries, item edits, uploads, restart/shutdown, import/export
  src/manifest.rs       portable export format (manifest.json + images/)
  src/jellyfin.rs       read-only export from a Jellyfin server into that format
  src/settings.rs       key/value settings (transcode.hwaccel, transcode.device)
  src/upscale.rs        AI upscale job queue, worker API, upscaled versions
  src/narration.rs      narration (thuyết minh) job queue, worker API, extra audio/subtitle tracks (§9)
  Dockerfile            web build + server build + Debian runtime (ffmpeg, mesa VAAPI)
  docker-compose.yml    the mini PC deployment
src/                    SolidJS client
  api.ts                API client, server profiles, auth, formatting helpers
  playback.ts           playability decisions, quality ladder, bandwidth test, VTT parser
  cast.ts               Chromecast sender + AirPlay
  native.ts             Android fullscreen + landscape (JavaScript bridge)
  update.ts             app update check / install (not on the web)
  pages/                Home, Library, Item, Player, Settings (5 tabs), Search, Login
  components/           TopBar, Row (shelf), Card, Icon (SVG), FolderPicker
src-tauri/              Tauri shell; src/proxy.rs = local header-adding proxy, src/update.rs = updates
upscaler/               experimental GPU upscale worker (Python + ffmpeg), Dockerfile, GPU compose, models/
narrator/               narration worker (Python: requests, numpy, edge-tts + ffmpeg), Dockerfile, README, and the compose
                        for the whole GPU side (Whisper, Argos, VieNeu x2, EasyOCR, nginx :8080, the worker), nginx/
  android/apply.sh      patches the generated Android project (cleartext http, gradle repos)
scripts/
  set-version.sh        sets the version in every manifest and lockfile
  build-mobile.sh       android | ios  (local builds, same steps as the mobile workflow)
  build-desktop.sh      linux | linux-arm64 | windows | macos
  desktop.Dockerfile    Ubuntu 22.04 build image (glibc 2.35) with cargo-xwin + NSIS
  publish-release.sh    branch release/vX.Y.Z + tag + GitHub release + upload dist-*/
.github/workflows/      build.yml, release.yml, mobile.yml (see §5)
docs/screenshots/       README screenshots (JPEG), taken with Playwright + Chrome from the live server
```

---

## 4. How it works (and why)

### 4.1 Data model (`server/migrations`)

- `libraries` (+ `library_paths`): one row per show/collection, like the Jellyfin setup (186 libraries).
- `items`: one row per video file **per library**, keyed by `(library_id, path)`.
  `title_locked` / `image_locked` mark user or Jellyfin-imported values that scans must not overwrite.
  `sort_title` is a natural-order key: accents folded, leading article dropped, digit runs zero-padded
  (`util::sort_key`) — it also powers search.
- `streams` is the ffprobe stream list as JSON, including the video `tag` (hvc1/hev1 matters, §4.4).
- `user_data`: position, played, favorite, last_played per user and item.
- `settings`: key/value. `users.avatar_ver`, `*.image_ver`: cache-busting versions for image URLs.

### 4.2 Scan and images

`scan.rs` walks each library folder, inserts new files, deletes vanished ones (a library whose folders are all
missing is skipped, so an unmounted disk does not wipe it), probes new/changed files with ffprobe (4 workers),
then frame-grabs a thumbnail at 10% for items with no image.

**Black bars** (2.0.6): after probing, `ffmpeg cropdetect` runs on 24 frames at 15/35/50/65/85% of each unchecked
video; the samples are united (a dark scene cannot over-crop, black frames are ignored) and stored in `items.crop`
(`NULL` unchecked, `''` none or under 1%, else `w:h:x:y`). Items with bars get their thumbnail grabbed again,
cropped, unless the image is custom/imported (`image_locked`; "Auto image" on the item redoes it cropped). The
player zooms the `<video>` with a CSS transform so the picture fills the screen (works for direct play, remux,
HLS and AI versions, the file is never touched). The zoom needs `videoWidth`, which iOS native HLS only knows after the first frame: it is re-measured on the video's `resize` event. A crop button in the top row turns it off (`localStorage.crop`).
A changed file (size/mtime) is checked again.

Images are rendered **once** into WebP `s/m/l` and served from `/img/...` with
`Cache-Control: immutable` + `?v=<version>`, so a client downloads each image once. This replaced the Jellyfin
problem of slow/missing thumbnails; Redis was proposed by the owner but is unnecessary (disk + page cache +
HTTP cache is faster than a network hop).

### 4.3 Migration from Jellyfin

`kellyfin jellyfin-export` reads Jellyfin's API (libraries, paths, titles, `DateCreated`, primary images,
per-user progress) into `manifest.json` + `images/`. `kellyfin import DIR [--map FROM=TO]` applies it and scans.
Result on the real server: 186 libraries, 6923 Jellyfin items, 7108 images; the scan found **897 more files**
Jellyfin had never indexed (7820 total). Jellyfin users have no exportable password: imported users
(`koi`) need `kellyfin passwd`.

**Wrong titles/posters inherited from Jellyfin.** Jellyfin's online metadata (TheMovieDb) matched generic episode
file names to unrelated films — `TẬP 5.3.mp4` in "Truyền Thuyết Jumong" became "Bảy Viên Ngọc Rồng: Tập Phim Về
Bardock" with that movie's poster. Import locks imported titles/images, so scans keep them. `repair.rs`
(`kellyfin repair [--apply]`, Dashboard → "Wrong titles and thumbnails"):
- *certainly wrong* = the file name has no meaningful word (only episode words/numbers) but carries a title:
  bulk-fixed back to the file name + a frame grab. On the real data: 6 items, all in Jumong, all wrong.
- *check yourself* = the title shares no word with a meaningful file name (39 items): mostly correct Vietnamese
  translations (Final Destination → "Lưỡi Hái Tử Thần"), a few real mismatches ("Tổ Quốc Trong Tim" → "Full
  Moon Rising"). Listed only, with a per-item "Use file name" button.
- A "portrait poster on a landscape video" rule was tried and dropped: it flagged 501 posters, mostly correct
  ones of real films (Totoro, Grave of the Fireflies).

### 4.4 Playback decision (`src/playback.ts`, `pages/Player.tsx`)

Order: **direct → remux → HLS transcode**, with automatic fallback on errors.

1. **Direct play** (`/items/{id}/file`, HTTP range): when the browser's `canPlayType` accepts container +
   codecs, the wanted audio track is the file's default one, and the MP4 index (moov) is ≤ 4 MB.
   *Why the moov rule*: a 1h45 movie had a 10 MB moov; the browser must fetch it before the first frame,
   ~4 s over WireGuard. A copy-remux starts in <1 s.
2. **Remux** (`/items/{id}/remux?start=&audio=&copy_audio=`): `ffmpeg -c:v copy` to fragmented MP4 over a
   chunked response; used for MKV, non-default audio tracks (4070 files have several), big moov.
   Seeking restarts it at a keyframe found by `/keyframe`. Audio is passed through when the client can decode
   it (Opus/AAC), else AAC stereo.
3. **HLS transcode** (§4.5) for codecs the device cannot decode (HEVC on Chrome/Linux, AV1 on old phones) or
   when the user picks a lower quality.

**Apple WebKit rules** (Safari, every iOS browser, Tauri on Apple):
- HEVC tagged **`hev1`** plays sound over a black picture; only `hvc1` shows video. The server records the tag
  (back-filled on first open for old rows) and such files go to transcode.
- WebKit refuses `<video>` sources without byte ranges, so remux is never used there.
- **Never a `%` height inside an `aspect-ratio` box** (`.thumb img`, `.lib-thumb img`): WKWebView (macOS app, iOS) then paints the last row of a grid
  far from its layout position (blank row, a card drawn over the next folder). Make the image `position: absolute; inset: 0` (2.19.1; the first
  sighting was the 17000 px row of 2.0.1).
- **Reproduce WebKit layout bugs in an embedded `WKWebView`** (`scripts/wk-repro/`, runs on mabu): Safari 18.6, Chrome (DPR 2) and WebKitGTK showed nothing
  for 2.19.0's bug; the harness showed it on the first load. `screencapture` over ssh returns only the wallpaper (no screen-recording
  permission): the harness captures its own window with `CGWindowListCreateImage`. Do not screenshot mabu's whole screen: it is the owner's desktop.
- These two rules are **not yet verified on a real iPhone** (WebKit could not run headless on koi-pc).

Subtitles are fetched as WebVTT and drawn by the player (not `<track>`), so they stay in sync when a remux
restarts mid-file. External `.srt/.vtt/.ass` next to the video (`name.vi.srt`, …; 13 457 of them) are found at
request time; embedded text tracks are extracted once and cached. Image subtitles (PGS) are listed as
unsupported.

Progress is saved every 10 s, after seeks, on pause, and on `pagehide`/hidden (tab close never runs cleanup).
≥ 92% marks the item played. Auto-next goes to the next item by `sort_title`, or to the first item of the next
season's library (`recommend::next_season`: "Arrow Season 1" → "Arrow Season 2").

Player controls are icons with popup menus (Jellyfin-style UX): favorite, CC (subtitles, with a "Secondary
subtitles" sub-menu), audio track, volume (slider on fine pointers only), quality (gear), next, cast, AirPlay, PiP,
fullscreen. **Dual subtitles** for language learning: a second track is drawn above the first, smaller and
yellow; its language is remembered (`sub2Lang`) and never equals the primary track.
**Playback speed** 0.25–2× sits under the gear menu ("Playback speed ›"), keys `<`/`>` step it; it is remembered
(`rate`) and set as `defaultPlaybackRate` so remux restarts and quality switches keep it.

Touch layout: play/pause and ±10 s sit in the middle of the video (shown with the controls), cast / AirPlay / PiP on
the top row, and the bottom row (clock, favorite, CC, audio, volume, quality, next, fullscreen) fits one line on a
375 px phone. Since 2.0.9 the centre play/±10 s overlay is used with a mouse too, and previous/next episode sit on both sides of the clock.

Mobile gestures: double tap a side = ±10 s (the skip icons carry a "10"). The brightness/volume swipes (1.5.0) were
removed in 2.0.4 at the owner's request: on iOS a double tap zoomed the page and the swipes then took every
gesture, so it could not be zoomed back. Page zoom is now off (`maximum-scale=1, user-scalable=no`,
`touch-action: manipulation` on the player).
- Android app: `window.KellyfinNative` (`MainActivity.kt`, `NativeBridge`, injected in `onWebViewCreate`) only does
  fullscreen = hidden system bars + `SENSOR_LANDSCAPE` even with auto-rotate off (back to `UNSPECIFIED` after).
  Forcing landscape with rotation lock on is not implemented on iOS.

### 4.5 Transcoding (`server/src/transcode.rs`)

- Master playlist: one H.264 rendition per rung from the source size (max 1080) down to 144p.
  Variant playlists list fixed-length segments for the whole file; segments are produced **only when
  requested**. A request > 6 segments ahead of the encoder (seek, quality change) restarts ffmpeg there.
  The encoder is SIGSTOPped when 12 segments ahead and resumed below 6; idle jobs die after 90 s.
- `-copyts -start_at_zero` keeps source timestamps so segments from different ffmpeg runs line up.
- **Segment length = `round(fps*4)` frames with a fixed GOP.** `h264_vaapi` ignores `-force_key_frames`;
  with it segments came out 25 s / 9.5 MB and seeks took 12 s.
- **VAAPI: CPU decode + `hwupload,scale_vaapi` + `h264_vaapi`.** Measured on the Vega for 20 s of 1080p HEVC
  → 720p: CPU decode 3.9 s vs VAAPI decode 6.0 s.
- **NVENC**: `-hwaccel cuda`, CPU scale, `h264_nvenc -preset p4`; verified on the RTX 3060 in Docker.
  Needs nvidia-container-toolkit and the commented `deploy:` block in compose.
- `transcode.hwaccel`: `auto` (NVIDIA, else VAAPI, else x264) | `nvenc` | `vaapi` | `none`. A hardware job
  that dies before its first segment is retried once with x264.
- Client "Auto" quality: hls.js ABR is **disabled** (fixed levels only). hls.js read transcoder start-up time as
  a slow network and thrashed 1080p↔144p, each switch restarting ffmpeg. Instead Auto measures bandwidth once
  (`/api/speedtest`, 2 MB) and caps at the player's pixel height; two stalls within a minute step down a rung.
- Measured: HEVC start ≈ 3.4 s, seek ≈ 3.7 s, then buffering 4–8× faster than real time.

### 4.6 Suggestions and search (`recommend.rs`)

Titles are folded and tokenized, dropping noise words (`tap, phan, season, …`). Related libraries rank by
"same show" (one token set contains the other) then Jaccard overlap; similar videos exclude libraries already
suggested. Search requires every query word in `sort_title`, with digits padded so "tap 1" ≠ "tap 10".

### 4.7 Apps, server profiles and the header proxy

The apps **and the web build** store several servers `{name, url, headers}` and a session per server; the
avatar menu lists them for a one-tap switch (straight in when that server still has a session). In a browser the
page's own server is the fixed first entry (`HERE`, session key `token`), others need CORS (the API allows any
origin); servers that need headers are refused on the web, since only the app's proxy can add them.

**Plain http in Apple/Linux apps**: those apps load the UI from `tauri://localhost`, which WebKit treats as a secure
origin, so `http://` servers are blocked as mixed content (ATS keys in Info.plist do not help). Any `http://` server
is therefore routed through the same local proxy as header servers when `location.protocol !== "http:"`;
`127.0.0.1` is allowed, and the Rust proxy is not subject to ATS. Android/Windows apps (`http://tauri.localhost`)
connect directly. Versions: `__APP_VERSION__` (Vite define from package.json) and the server's `/api/health`
are shown on the sign-in and add-server screens, in the avatar menu and in Settings → Profile → About. If a server has headers
(e.g. a Pangolin access token), `src-tauri/src/proxy.rs` serves `http://127.0.0.1:<port>/<random secret>/…`
and forwards everything — API, images, video, HLS — adding the headers, streaming both ways with ranges.
Reason: `<video>`/`<img>` cannot send custom headers. Tested by a unit test only; **not yet run inside a real app**.

**iOS suspension** (2.0.2): after the app sits in the background, iOS reclaims its listening socket, so every API call
and image failed (blank Home, "?" thumbnails; Android does not do this). `set_server` now checks the listener
(local `OPTIONS` → 204, 1 s timeout) and serves on a new port if it is dead; `api.ts` calls it on every return to
the foreground and reloads the page when the base changed. Pull down at the top of any page reloads it, and
Home shows an error with Retry instead of a blank page. Inferred from the symptoms and Apple's TN2277; not
reproduced on a device yet.

### 4.7a Navigation

- Library bar like Jellyfin's: as many library chips as fit (widths are summed, so the result does not depend on
  the current layout), the rest under **More ▾** — a plain list with an accent-insensitive filter. The More button
  always keeps its place (hidden when unused) so the bar's width never changes with the result. A sidebar was
  tried in 1.3.0 and replaced by this at the owner's request. On phones the chips are hidden and the logo opens
  the full list.
- A ← button on every page but Home (`history.back()`, or Home when there is no history): the only way back
  on iOS and in the web app on phones.
- Android app back gesture (`MainActivity.kt`): goes back through the WebView's history; with none left it shows
  "Swipe back again to exit" and leaves on a second back within 2 s.
- Library pages: grid (cards) or list (small thumbnail, title, length, resolution) view, one button cycling
  between them, remembered (`libView`).
- Top bar height is `56px + var(--safe-top)` so the notch/status-bar padding no longer squeezes the content
  (it was cut on iOS); `native.ts` sets `--safe-top/--safe-bottom` to 0 in the Android app, whose WebView is
  already padded by the system bars.
- iOS layout bugs: test in the iOS Simulator on mabu (`xcrun simctl`, iPhone 15 / iOS 17.2) — Playwright WebKit
  and Android do not reproduce them. A small proxy on mabu (`127.0.0.1:9077` → the server) can set the token in
  `localStorage` and inject a probe script that POSTs computed layout back; `simctl io booted screenshot` to see it.
  Strip `Accept-Encoding` upstream or the HTML arrives gzipped and cannot be patched.

### 4.7b Mobile keyboards and Android cleartext

- On-screen keyboards overlay the page (iOS; Android edge-to-edge). `src/viewport.ts` measures the covered height
  from `visualViewport` into `--kb` (bottom padding on `body`) and scrolls the focused field to the centre; the
  viewport meta has `interactive-widget=resizes-content` for Chrome; the login page uses `100dvh`.
- Android app: `src-tauri/android/MainActivity.kt` (copied by `apply.sh`) pads the WebView by the IME and system-bar
  insets, because edge-to-edge ignores `adjustResize`.
- **Plain http in the Android app**: Tauri's template sets `usesCleartextTraffic` with a Gradle placeholder that is
  `"false"` for release builds, so v1.0.0 could not reach `http://24.8.0.2:8024` (Chrome could). `apply.sh` now flips
  the placeholder and fails if it is still false; `mobile.yml` checks the signed APK's manifest with `aapt`.

### 4.7c App updates

`src-tauri/src/update.rs` (from Kermius) + `src/update.ts`: checked 5 s after start (banner "Kellyfin X is
available" with Update / Later) and by "Check for updates" in Settings → Profile → About.
- Desktop: `tauri-plugin-updater` reads `releases/latest/download/latest.json`, verifies the minisign signature with
  the public key in `tauri.conf.json`, installs and restarts. `release.yml` signs bundles with the
  `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` secrets (`createUpdaterArtifacts: true`) and the publish job assembles
  `latest.json` from every platform's `.sig`; it fails the run if no signed bundle exists (missing secrets).
- Android: compares with GitHub's latest release and opens the APK download (`open_release`, restricted to this
  repo's URLs); the mabu dashboard also auto-installs. iOS: only reports; the re-sign job installs.
- The web build never checks (the server is updated with Docker).

### 4.8 Cast

Chromecast uses the Cast Web Sender SDK (Chrome only, not Tauri webviews): the TV fetches the original MP4 if
it is plain H.264 with the default audio, else the H.264 HLS transcode, plus the chosen subtitle. While casting,
the player controls drive the TV; ending the session resumes locally. AirPlay uses WebKit's picker.
Limits: the TV must reach the server URL (a WireGuard-only IP will not work), header-protected servers cannot be
cast, untested with a real receiver (none was found on the mini PC's LAN).

---

## 5. Build, deploy, release

### Versioning

SemVer: a minor per feature set, a patch per fix — **every shipped change bumps it** (owner's rule).
1.0.0 was the first release (0.x was development). Use `scripts/set-version.sh X.Y.Z`; it updates `package.json`, both `Cargo.toml`/`Cargo.lock`,
`tauri.conf.json`. Images: Docker Hub `hienanh/kellyfin` (`:latest` + `:X.Y.Z` from main; the old `:dev` tags are no longer updated).

### CI is the build system

GitHub Actions builds everything (it was briefly disabled during an Actions outage on 2026-10-19 and is back on):
- `build.yml` — every push to `main` (and pull requests): web build, server tests, Tauri tests, multi-arch image
  (amd64 and arm64 on native runners, merged into one manifest) pushed to `hienanh/kellyfin`.
- `release.yml` — push to `main`: if `v<package.json version>` has no tag yet, creates `release/vX.Y.Z`, the tag,
  desktop installers (Linux x64/arm64, Windows x64/x86/arm64, macOS arm64/x64), and calls `mobile.yml`.
- `mobile.yml` — Android APK (signed with secrets `ANDROID_KEYSTORE_B64/PASS`, else a throwaway key) and an
  unsigned iOS IPA; push to `ci/mobile` to test it alone (assets go to the `mobile-dev` pre-release).

Deploy the server after CI: `ssh mini-wg 'cd ~/Software/kellyfin && docker compose pull && docker compose up -d'`.

Local fallback only if Actions is unavailable (the owner prefers CI — ask first):

```sh
# Rust without a local toolchain (koi-pc)
docker run --rm -u $(id -u):$(id -g) -e CARGO_HOME=/cargo -v <cache>:/cargo -v $PWD/server:/src -w /src rust:1-bookworm cargo test

# Server image -> mini PC (no Docker Hub push)
docker build -f server/Dockerfile -t hienanh/kellyfin:latest .
docker save hienanh/kellyfin:latest | gzip -1 | ssh mini-wg 'gunzip | docker load && cd ~/Software/kellyfin && docker compose up -d'

# Mobile, on mabu (repo clone: ~/Downloads/Miscellaneous/Kellyfin)
scripts/build-mobile.sh android   # signed APK -> dist-mobile/ (verified: builds)
scripts/build-mobile.sh ios       # unsigned IPA -> dist-mobile/ (not run to completion yet)

# Desktop
scripts/build-desktop.sh linux    # koi-pc, Docker Ubuntu 22.04 (verified: deb, rpm, AppImage)
scripts/build-desktop.sh windows  # koi-pc, Docker Ubuntu 24.04 + cargo-xwin (NSIS 3.08 lacked RestartManager.nsh)
scripts/build-desktop.sh macos    # mabu, universal .dmg + .app.tar.gz (not run yet)
scripts/build-desktop.sh linux-arm64  # needs QEMU binfmt on the host (not installed)

# Publish local builds: copy mabu's dist-mobile/ next to dist-desktop/, then
scripts/publish-release.sh        # release/vX.Y.Z branch, tag, release, uploads (token: ~/.config/kellyfin/github-token)
```


### IPA re-sign (mabu, `~/Downloads/Miscellaneous/ipa-resign-dashboard`, see its `docs/JOB-SPEC.md`)

Registered in the dashboard (`http://<mabu>:7877`) on 2026-10-20:

- `~/ipa-resign/jobs/kellyfin-android.json` — artifact mode, GitHub release `\.apk$`, `install.package
  com.koi.kellyfin`, `managed: true` (every 12 h), `autoUpdateOnConnect`. First run: **`install ok 1.0.0`** on the
  A8 Star (Samsung SM-G885F, USB serial `486c6a0a`). The release APK is signed with the release keystore
  (cert SHA-256 `DD:81:C1:…:DF:AD`), so later versions update in place.
- `~/ipa-resign/jobs/kellyfin.json` — script mode, `managed: false`, `launchdLabel com.datlt4.kellyfin-resign`,
  `forceArg --force`, `renewArg --renew`, artifact `\.ipa$`, `install.package com.datlt4.kellyfin`.
  Script `~/kellyfin-ios/kellyfin-ios-resign.sh` (from Kermius'): downloads the newest release IPA, keeps a 7-day
  free-account profile via the helper project `~/kellyfin-ios/helper` (Kermius' helper with the bundle id
  changed), re-signs, installs, prints the dashboard markers. Agent `~/Library/LaunchAgents/com.datlt4.kellyfin-resign.plist`
  (RunAtLoad, every 6 h) is loaded. First runs: **`no iPhone reachable`** — the iPhone 11 Pro Max is paired over
  Wi-Fi and was `unavailable`; it needs to be unlocked near the Mac (or on USB), then "Resign ngay" in the dashboard.
- A free Apple ID allows 3 sideloaded apps per device; Kermius already uses one.

---

## 6. Open items

1. **Run the repair on the server** (Dashboard → Check → Fix): 6 wrong Jumong items; review the 39 others.
1b. Start an upscale worker permanently on koi-pc (`upscaler/docker-compose.yml`) when
    trained models exist.
2. **Finish P5**: v1.0.0 is out and installed on the A8 Star; the iOS job needs one successful run with the
   iPhone reachable (unlock it near mabu or plug USB, then "Resign ngay"); first iOS install also tests the
   free-account 3-app limit.
3. **Linux arm64 desktop** is covered by CI (`ubuntu-22.04-arm`); locally it would need QEMU binfmt.
4. **Verify on real devices**: hev1/remux rules on iOS, header proxy in the apps, Android WebView fullscreen
   and PiP (may need a native plugin), Chromecast/AirPlay.
5. Disk on koi-pc: Rust/Docker build outputs were deleted on request (~19 GB). `server/target`,
   `src-tauri/target` and the `rust:1-bookworm` image come back whenever tests run locally — delete them after.
   Docker's build cache (~75 GB, mostly other projects) was left alone.
6. User `koi` (imported from Jellyfin, id 1) disappeared from the server between 2026-10-19 and 10-20; the
   server has no request log, so when/how is unknown. It can be recreated from the Jellyfin export (9 progress
   entries; password must be set again) — waiting for the owner.
7. P7 next steps (§8): trained models, live upscaling.

## 7. Commit history (what each step did and what it taught)

| Commit | Version | What | Notes / lessons |
|---|---|---|---|
| `69f8387` | – | init (CLAUDE.md) | |
| `659e7ea` | 0.1.0 | P0+P1: server, SolidJS client, Tauri shell, Docker, CI, MIT license | Rust built in Docker (no local toolchain). Import + full scan of the real library took 25 min (one-off). |
| `daecbdd` | | Tauri `Cargo.lock` | |
| `bd5453f` | | item backdrop gradient | found by screenshot review |
| `5f2dc80` | | hover outline clipped by `overflow-x` shelves | |
| `f0b185f` | (0.2.0) | P2 player: direct, remux, subtitles, progress | Measured the 10 MB moov problem → remux rule; fixed lost seeks while a remux was loading (`pendingAt`); progress saved on tab close. |
| `3ac1564` | | cleanup | |
| `f02026e` | 0.2.0 | Docker Hub `hienanh/kellyfin`, versioning script | |
| `07da1ad` | 0.3.0 | P3 HLS + VAAPI | 25 s segments (forced keyframes ignored) → fixed GOP; VAAPI decode slower than CPU on Vega; hls.js ABR thrash → bandwidth-picked fixed levels. |
| `ab04b74` | 0.4.0 | P4 settings/dashboard/users/libraries/search/suggestions | Folder picker buttons submitted the form (fixed); media files are never touched by deletes. |
| `b622096` | 0.4.1 | CI multi-arch build race | amd64/arm64 shared cargo cache mounts ("File exists"; could also mix architectures) → per-platform locked caches. |
| `e72bc6a` | 0.4.2 | iOS black video (hev1), SVG icons, NVENC, native arm64 CI | ffmpeg stderr was piped and never read (would stall ffmpeg) → inherited. |
| `c2a71ee` | 0.5.0 | server profiles + header proxy, swipe gestures, release/mobile workflows | |
| `d4fc55d` | 0.6.0 | Chromecast + AirPlay | |
| `0d0f16d` | | local mobile build script | BSD vs GNU `sed -i`. |
| `e37f596` | | local desktop build script, executable bits | The repo disk does not keep exec bits: use `git update-index --chmod=+x`. |
| `52979d7` | 0.6.1 | proxy compile fixes, Docker builds keep host `node_modules` | The proxy had never been compiled; first real build caught it. |
| `00b81e1`, `23d76ae` | | this handoff, `publish-release.sh`, Windows local build on Ubuntu 24.04 | Ubuntu 22.04's NSIS 3.08 lacks `Win\RestartManager.nsh`. CI then went green for build + mobile (first APK/IPA). |
| `bd4c505` | 0.7.0 | icon controls with menus, dual subtitles, search icon redrawn | Hand-written SVG paths can render wrong: render every icon to a contact sheet to check. |
| `c8fe770` | 0.7.1 | repair wrong titles/posters from Jellyfin | Dry-run on real data first: the first rule would have replaced 501 correct posters. |
| `7b2e64e` | 0.8.0 | playback speed 0.25–2× | A src change resets `playbackRate` to `defaultPlaybackRate`: set both. |
| `80e2f07` | 2.0.0 | in-app updates (signed desktop updater, APK link, iOS notice), AI upscale pipeline + worker, back button on the server picker, Jellyfin retired | Drive the updater from Rust commands (no extra webview permissions); the upscale worker pulls work so the GPU PC needs no inbound access. Released: 34 assets incl. 13 `.sig` and `latest.json` covering all 20 desktop platform keys; A8 Star updated 1.5.0 → 2.0.0. |
| `b408b45` | 2.0.1 | iOS: Settings → Libraries rows blank, "AI upscale" tab wrapped | Safari 17 sized a grid row ~17000px because an `img` with `height: 100%` sat in an `aspect-ratio` grid item (content drawn far below, the visible row empty); the image is now absolutely positioned. Tabs `nowrap`. Android did not show either. Released (34 assets, CI green). |
| (2.13.0 commit) | 2.13.0 | OpenRouter OCR as a second engine on the OCR node (pipeline editor: Engine → OpenRouter OCR); 4 frames per request (`OPENROUTER_BATCH`), JSON array back, `null` for a frame without text; per-user API key stored AES-256-GCM-encrypted (`user_openrouter`, key from `KELLYFIN_OPENROUTER_SECRET` in the server's .env — decryption drops legacy/encypted-as-plain rows), settings → profile stores key + primary + fallback model, pick from the live OpenRouter `/models` list filtered to free (prompt=0, completion=0), vision-capable, non-embedding models; a job whose pipeline has an OpenRouter OCR node checks the key and both models at claim time — missing key or a model that has stopped being free fails the job with a visible error instead of charging the user | Owner after the OpenRouter OCR PoC on Smallville S2 E18 (burned-in Vietnamese subs, `dots-3-note-preview:free` read every line, ~4 s/batch, 104 requests, ~91 % fuzzy match against the broken `.vi.srt`, which the owner accepted as good enough): "hãy thêm OpenRouter OCR như một loại engine node riêng, Batch size: 4 ảnh/request như thử nghiệm" + "cho chọn model chính và model fallback" + "nen cho chọn model vì các model free có thể sẽ không còn free nữa, khi nó không còn free nữa thì hãy ngăn lại, thông báo lỗi". Checks: `cargo test` (19), `tsc`, vite build, worker `py_compile`. Not clicked through in a browser; the per-job config switch (engine="openrouter" on the OCR node) is learned from the claim response (new Claimed.config fields, key stays server-side with the worker). Tested live on the mini with a DB copy (2026-11-06, test stack on film-sub_default, item = a 103 s cut of the Smallville episode): login → /me/openrouter (enabled:true), 10 free vision models listed, a paid model rejected with 400 ("primary model \"openai/gpt-4o\" is not on the free list (or is not vision-capable)"), key saved AES-256-GCM-encrypted (136 chars base64 in the DB, never plaintext), preset with engine openrouter → OpenRouter read 22 lines (the dots-3-note-preview model hits AtlasCloud HTTP 400 on ~1 batch per run; the nemotron fallback silently covers those batches), TTS via edge-tts (6 of 22 lines fell back to Google TTS), and the whole chain passed once 2.14.0 fixed the no-audio mux bug: video replaced, `.vi.srt` written, duration 95.234 s (the 103 s cut is 95.234 s probed), the Vietnamese audio track tagged `vie`. |
| (2.19.5 commit) | 2.19.5 | Security review fixes: session tokens stored hashed (SHA-256) with 90-day expiry, revoked on every password change, login throttle (429 after 5 fails); `?t=` scoped to media bytes only (new `MediaUser`, API needs the header); `/img` requires login (frontend sends `?t=`); CORS allow-listed to tauri/capacitor/loopback origins; library roots reject `/`, `/proc`, `/etc`, `..`; manifest item paths must sit under their library and image refs under `images/`; scanner resolves symlinks and stays inside roots; OpenRouter secret stretched with a versioned domain KDF (`v1` blobs, legacy blobs still decrypt); password minimum 8; DB dir `0700`, DB files `0600`; worker uploads capped (512 MB narration, 32 GB upscale); transcodes capped at 12 jobs; Tauri proxy echoes CORS only to app origins, no redirects, constant-time secret compare, 64 MB cap; webview CSP; `.env` gitignored and loaded by the server | Checks: `cargo test` (19), `tsc`, src-tauri `cargo check`, migrations 0001->0010 apply clean on a blank DB. Released; needs a re-login everywhere. |
| `ab6a2e1` | 2.19.4 | worker `whisper()`: the `/health` check waits (15 s steps, 8 times, heartbeat each) while the service answers 502/503/504 or refuses the connection, other HTTP errors still fail at once, a timeout still means busy; the transcribe call is asked again (3 tries, 30 s apart) after a 502/503/504 or a lost connection, cancelling the earlier request id first so the service does not run it twice; `nginx/default.conf.tpl`: `resolver 127.0.0.11 valid=5s` and `set $whisper/$argos` + `rewrite ... break` + `proxy_pass $var` for `/whisper/` and `/translate/` (easyocr stays a plain name: on the mini it comes from `extra_hosts`, which Docker's DNS does not read) | Follow-up to the 502 window of the 2.19.2/2.19.3 deployment (jobs 2, 3, 4 failed within seconds). Tested: worker with 8 fake-service cases (restart at the health step, at the transcribe step, lost connection, 404 still fatal, 502 forever gives up, deletion during the wait); the real template with real containers: `/whisper/health`, `POST /whisper/v1/transcribe` (HTTP 200, same 1483 bytes), `/translate/...` and the cancel call route correctly, and after recreating the whisper container (IP 172.22.0.2 -> .6) nginx kept answering with no reload. The mini's own `nginx/default.conf.tpl` is a copy (easyocr on :8890 via the Jetson `extra_hosts`): it needed the same two edits by hand (done on the mini, backup `nginx/default.conf.tpl.bak_before_resolver`). Released; deployed on the mini (nginx, whisper, worker, server recreated). |
| `9bd8294` | (no version) | the patched Whisper server is an image, not a file to mount: `narrator/whisper/Dockerfile` (`FROM hienanh/whisper:latest` + `server.py`), `.github/workflows/whisper.yml` (push of `narrator/whisper/**` or by hand -> `hienanh/kellyfin-whisper:latest` and `:<package version>`); both compose files use that image and no longer mount `server.py` | Owner: "why not rebuild the whisper image". It was mounted because `hienanh/whisper` is the owner's own image that this repo's CI does not build and cannot push to; a thin layer under a new name needs no access to it. Tested locally: the built image answers `/health` and passes the same cancel test (running request stopped 0.6 s after the call, queued one at once). Built and pushed by CI (anonymous pull works). Deployed on the mini (compose backup `docker-compose.yml.bak_before_whisper_image`, the old `whisper/server.py` there is unused). **(2.19.4 makes that unnecessary, see its row)** After recreating the whisper container run `docker exec fs-nginx nginx -s reload`: nginx resolves `whisper` once at start, so it kept the old container IP and answered 502 for ~25 s, during which the worker (its `/health` check fails on anything but a timeout) failed jobs 2, 3 and 4 within seconds each (owner must Retry). |
| `8099bbb` | 2.19.3 | Whisper runs can be cancelled: the worker sends `X-Request-Id` with the transcribe call and, when its job is deleted or paused (`JobCancelled`), `POST /whisper/v1/cancel/<id>`; `whisper/server.py` stops a waiting request at once (409) and a running one at the next segment | Owner (screenshot of 001 "running 0 %" after re-queueing 001-005): two requests were in the Whisper container, found by their temp files in `/tmp` (106 MB wav = ~55 min of audio each): the run of the deleted job 004 (06:08:00) still holding the CPU at 333 % and 001's (06:10:57) queued behind it, i.e. minutes of work nobody wanted; a transcription cannot be stopped otherwise (2.5.4 only dropped the worker's wait). Tested against the real image on a 9-minute file: a running request cancelled 1.9 s after the call (409), a queued one at once while the other kept running, a normal request right after; worker: the cancel call carries the same id and a failing cancel call does not hide the cancellation. A service without the mount answers 404 to the cancel call: nothing changes there. Released (34 assets); not yet deployed on the mini (a restart of whisper there was refused by the permission classifier while jobs ran: the owner decides). |
| `76693d5` | 2.19.2 | `narrator/whisper/server.py` (the `hienanh/whisper` image's `server.py` with `transcribe` as a plain `def`, one run at a time under `_lock`, `/health` that does not load the model) mounted over `/app/server.py` by both compose files; worker `whisper()`: a `/health` that times out no longer fails the job (it waits behind the busy service), 502/503/504 on the transcribe call are asked again up to 3 times 30 s apart | Owner's queue (CPU, mini): 001 `504 Gateway Time-out ... /whisper/v1/transcribe`, 002 and 003 `Whisper at .../whisper/health does not answer: Read timed out (read timeout=15)`. The image's `async def transcribe` runs the transcription in the event loop: measured with a 70 s speech file, a `/health` sent during the run waited ~4 s with the image's file and answered in 0.0 s (7 of 7) with the patched one, same 1483-character transcript. On the mini a medium run is minutes, so every health check and a second request waited behind it (a run a deleted/paused job left behind keeps going: 2.5.4). Worker cases tested with fake responses; deployed on the mini (server.py copied to `~/Software/kellyfin/whisper/` and mounted in its compose, backup `docker-compose.yml.bak_before_whisper`): during a real transcription (CPU 103 %) `/health` answered in 5-9 ms, and the whisper restart gave a 502 that the worker retried instead of failing job 004. |
| `b0eb4b3` | 2.19.1 | `.thumb img` is `position: absolute` (was `height: 100%` inside the `aspect-ratio: 16 / 9` `.thumb` grid item); `scripts/wk-repro/` (WKWebView harness, fake server, blank-band detector) | Owner, after updating macOS and iOS to 2.19.0 (sure of it): the tree view was still wrong, on the macOS app (and iOS). 2.19.0's `content-visibility` theory was wrong. Reproduced in a `WKWebView` on mabu (2048 px viewport, DPR 2, six columns, 124 cards in a folder tree like the owner's FreeCAD course, slow thumbnails): from the first paint the lone card of a folder's second row is missing and drawn over the next folder; the last row of folder 03 over folder 04's titles. Baseline: gaps up to 195 px between rows in 2/2 runs; with the absolute image 43 px (the normal folder gap) in 4/4 runs, and with the real build. Not reproduced in Safari 18.6 (static page), Chrome 156 at DPR 2 with the real app and the same data, nor on the iPad simulator (not tried to the end). Not looked at on a real iPhone. Released (34 assets, both macOS builds). |
| (2.19.0 commit) | 2.19.0 | Library tree on macOS: `content-visibility: auto` removed from `.grid > .card`, `.row-item` and `.row` — WKWebView could paint an off-screen (skipped) card at a stale offset and at its `contain-intrinsic-size` width, so one card of a folder landed ~1000 px lower (among the next folders) at 1-column width while its row stayed blank ("tree view bị đẩy tít xuống dưới", intermittent). | Owner's macOS screenshots (same FreeCAD library, 16:50 vs 16:51): folder "02-Getting started with FreeCAD 1.0 & it's Customization" has 7 videos — six cards in row 1, the 7th ("007 Get My Optimized FreeCAD Setting Files") painted below folders 03/04 and their subfolders, at 300 px wide, with a ~530 px empty band where its row should be. Ubuntu (WebKitGTK) and Chrome did not show it. Could not reproduce in a WebKitGTK 2.50 harness with the same CSS; the mispainted card is exactly the one that was off-screen at load, which is `content-visibility`'s skip path. Checks: `tsc`, vite build; tree/grid/list layout re-rendered at 2048×1280. |
| (2.18.0 commit) | 2.18.0 | OpenRouter OCR: fail fast on 429 (no more 45 s holds burning the shared daily free-model allowance, a shared 1000 requests/day across all free models), and a claim-time quota guard via `/api/v1/auth/key` — a job fails on the spot with a clear message when the key's free allowance for the day is used up (owner's Smallville 18–23 jobs died this way, using 1002/1000 free requests in one afternoon of retries on two action-heavy episodes; until today the jobs also slept 45 s per batch, doing it again). | Found when the owner's 6 OpenRouter OCR jobs all failed; quota readout confirmed `free_model_daily_requests.used = 1002`. Checks: `cargo test` (19), `py_compile`, worker probed the GET endpoint on the mini. |
| (2.17.0 commit) | 2.17.0 | "Check server and queue" blocked pipelines with an OpenRouter OCR node ("EasyOCR (burned-in subtitles): HTTPError: 502") because the client demanded the EasyOCR service unconditionally — the film-sub stack on the mini no longer runs EasyOCR at all; the OCR node's engine now selects the preflight dependency (openrouter → the new OpenRouter health probe reported by the worker, easyocr → EasyOCR as before). | Owner duplicated "OCR · edge-tts only (no VieNeu)" as an OpenRouter OCR preset (8 in the picker) and the preflight refused it. Checks: `tsc`, `py_compile`, vite build. |
| (2.16.0 commit) | 2.16.0 | Settings → Profile: the OpenRouter OCR model dropdown is no longer stuck on "No models loaded yet (needs a stored key)" — the model list resource re-runs when `key_set` flips and after every save (previously the single-argument `createResource` fetcher never re-evaluated); it also shows "Loading the free models…" / a clear error if the fetch fails. | Owner saved their key but the dropdown stayed on the placeholder. Verified the served 2.15.0 bundle contains the panel (so the UI is 2.16.0 as far as the panel is concerned) — the bug was the resource semantics, not the API or the network (models endpoint reachable, 453 models, 9 free vision listed from the kellyfin container's netns). Checks: `tsc`, vite build. |
| (2.15.0 commit) | 2.15.0 | Empty subtitles are not input: an external .srt (or a lesson's own) with 0 bytes is filtered from `external_subs` and `sub_map`, so queueing the 5 FreeCAD lessons with empty `.en.srt` now skips them instead of failing; the worker reports an empty source subtitle as such. | Owner asked why 5 "FreeCAD Course" narration jobs ("Argos returned the en text unchanged for vi") failed: the five 0-byte .en.srt files (downloaded 2026-11-05 17:31, all other lessons in the course have non-empty ones). Checks: `cargo test` (19; `finds_external_subs` now asserts a 0-byte file is not a track), `py_compile`, vite build. |
| (2.14.0 commit) | 2.14.0 | Narration: a video without sound (no audio track) is muxed again — the voice track is padded with endless silence and `-shortest` does not cut a stream-copied video, so the merged file came out longer than the video and the duration check failed the whole job (this killed every OpenRouter OCR test job; their hand-inserted test item had no streams, walking the no-audio path — which real videos without sound always take, so this was a live product bug); the mux output is now clamped with `-t <item duration>` | Found while testing 2.13.0 end-to-end on the mini (the reproduction command produced 97.64 s output from a 95.23 s video; one frame of ffmpeg and the whole picture cleared: `-shortest` is ignored for copied video). Verified on the mini: both containers recreated to 2.14.0, migration 0009 auto-applied on the real DB, and the test job (id 1) then completed — `.vi.srt` written, Vietnamese `vie` track added, duration 95.234 s exact, job `done`. Real mini: queue drained (135, 136, 137 all done) before both containers were recreated; the narrator rejoined with the same name. Checks: `cargo test` (19), vite build, ffmpeg before/after reproduction. |
| (2.12.0 commit) | 2.12.0 | the TTS stage can no longer hang: `EDGE_TIMEOUT` (20 s, `EDGE_TTS_TIMEOUT` override) per edge-tts line inside `tts_edge` (`asyncio.wait_for`) — a line Microsoft holds open forever is failed, not parked; fallback to **Google Translate TTS** (`client=tw-ob`; 200-char limit, split at word boundaries; 20 s for a line, retries 1/4/10 s; UA spoofed) whenever edge-tts cannot make a line, and the **whole job goes to Google** when the probe fails at stage start (the probe itself no longer falls back, so dead-edge jobs do not pay edge timeouts per line); every fallback is logged ("N of M lines read by Google TTS") and Google lines are timed identically on the timeline; smoke-tested in the running worker with a monkeypatched all-failing edge_tts (whole job -> Google in seconds, wav produced) and a mixed case (1 of 4 lines on Google, rest on "edge"), plus `py_compile` | Owner: "có vẻ đang bị treo, nhưng khong tự chuyển sang google dịch à?" — the job had been parked at 79.6 % for 4 h (`updated_at` 23:33:55, mini 03:42 UTC) with no logs and 2.5 % CPU: one line edge_tts never answered, the worker has no per-line timeout so the slot was held forever and the job was not stoppable (progress never fires a 410). Checks: `py_compile`, container smoke tests with monkeypatched edge_tts. Verified live on the mini right after redeploy: job 88 (previously parked at 79.6 % for 4 h) requeued itself, ran `edge-tts` with 20 s timeouts (5 of 135 lines fell back to Google), voice uploaded in 93 s and the job completed; job 89 started automatically. Both containers recreated (narrator 03:58:12 UTC). Release 2.12.0 published (34 assets), build green. |
| (2.11.0 commit) | 2.11.0 | Narration tab shows a note under the pipeline picker for a subtitle-input pipeline: a **skip** box (amber) listing the videos in the chosen library without the pipeline's input subtitles ("…they will be skipped", first 8 + "and N more") and a separate **lesson** box (blue) listing the subtitle files with no video or audio file ("audio only"), with a hint to the lessons toggle when it is off; lists come from a new `POST /admin/narration/input-check` (`{library_id, preset}`), videos use the same `sub_map` lookup as the claim/queue (relative paths), lessons the scanner's own `walk(…, true)` (made `pub(crate)`) | Owner after 2.10.0: "hãy hiện 1 note nhỏ là có những video nào không có subtitle trong folder vừa chọn … rồi những subtitle nào không có video -> chỉ xuất ra audio cũng có 1 đánh dấu riêng". Checked: `cargo test` (19), `tsc`, vite build, and an **end-to-end** run on a throw-away copy of the DB (test library with a missing-sub video, an embedded-mov_text-sub video, a `.en.srt` lesson in a subfolder): `subs-edge` → `{"videos_without_input":["002 B.mp4","005 E.mkv","006 F.mkv"],"subtitles_without_video":["003 C.en.srt","subfolder/006 G.en.srt"]}`, `whisper` → no skip list, same orphan list. Both containers were redeployed on the mini the same day (kellyfin & kellyfin-narrator recreated at 20:33, served bundle is 2.11.0, images 2.11.0 pulled). The `build` workflow's `tauri-check` job was cancelled by the owner after the images were pushed (harmless: images and manifest built fine, release 2.11.0 green, 34 assets). Not clicked through in a browser. |
| (2.10.0 commit) | 2.10.0 | queueing with a pipeline whose only source is a subtitle file (e.g. "English subtitles → Vietnamese voice (edge-tts) · audio lessons") now **skips** the items that have none of the subtitle languages it asks for, instead of queueing jobs that die with "the video has no en subtitles" — same subtitle lookup the claim does (`stream::subtitle_tracks`: external files beside the video + embedded text tracks, lessons count their own `.srt` as `self`; factored into `sub_map`). For the rest (incl. Whisper/OCR pipelines) the queue behaves as before. The response is now `{queued, skipped}`: the Narration tab note, the item Narration button and the library Narration button all say how many were skipped and why | Owner: "tôi chọn mode input english sub mà k kiếm đủ input … folder có 134 en.srt nhưng chỉ queue 119" and for the one video without en.srt ("003 Reorient Sketch" — the owner will run it with Whisper CPU): "fix lỗi pipeline 'input English sub' nhưng đầu vào vẫn tìm theo video đi". Checks: `cargo test` (19, incl. new `sub_map_finds_external_and_lesson_self`), `tsc`, vite build. The `desktop (linux_x64)` job was NOT stuck this time; the release published normally (34 assets, tag v2.10.0, `latest.json` present) and the mini was redeployed — served bundle is 2.10.0. Not clicked through in a browser. |
| (2.9.0 commit) | 2.9.0 | narration queue: a **Videos at once** setting in Settings → Narration (`narration.jobs`, `"1"` default = one video at a time, `"3"` = the old three at once); the worker reads it from the claim response's `config.jobs` (flattened, top level) and clamps it to its `WORKER_JOBS` env cap; "Clear paused" button (with a confirm) in the queue panel next to "Resume paused" — bulk `POST /admin/narration/jobs/clear` with `status="paused"` (server `clear()` now accepts paused in addition to done/failed) | Owner after edge-tts throttled the queue (intermittent `NoAudioReceived`, 5–9 s per line under 12-way concurrency from one IP; 25 % of lines failed the first pass): "thế đừng cho chạy song song nữa, hãy cho option chạy tuần tự hay parallel, mặc định luôn là chạy tuần tự xử lý từng video" + "hãy thêm nút xóa hết khi pause". Checks: `tsc`, vite build, `cargo test` (18), narrator `py_compile`. The narrator and server images were redeployed to the mini the same day (both containers recreated, worker logs "up to 3 videos at once (Kellyfin sets how many)", served bundle is 2.9.0). The `desktop (linux_x64)` job was stuck in `apt-get install` for 3+ hours (runner issue); it recovered and the release published later the same day — 34 assets, tag v2.9.0, desktop updates can pull it. Not clicked through in a browser. |
| (2.8.1 commit) | 2.8.1 | the tree list view is gone: the folder tree is always grid cards (a `"tree-grid"` saved in `localStorage` maps back to `"tree"`); folder rows are bigger and breathe — chevron 17px, 24px folder icon, 17px semibold name, 13px count, 15/11px padding, wider spacing between sections; grid/list/tree cycling button unchanged (tree icon is the folder-with-grid glyph) | Owner after 2.8.0: "bỏ hẳn tree list đi; icon folder và tên folder cần rõ hơn, cho folder to hơn và giữa các folder dãn cách xa hơn, để dễ nhìn hơn". Checked: `tsc`, vite build. Released (34 assets, build and release green on da21e3b, tag v2.8.1, served bundle reports 2.8.1); the mini was redeployed the same day and `kellyfin` was recreated on the new image. Not clicked through in a browser. |
| (2.8.0 commit) | 2.8.0 | library tree view nests folders to any depth, VS Code style: `GET /libraries/{id}/tree` returns one nested `Nested<Card>` root (`tree.rs::nest`; the entries are tree-ordered, so a folder is created after its parent, in natural order; collapse key = folder path relative to the library, all state kept on library switch); the videos follow the view: "tree" keeps the compact rows, the new "tree grid" (fourth state of the view button, folder-with-grid icon) shows them as cards; folder heads rotate a chevron and count everything under the branch; root-level videos are listed first as before; rows/grid inside `.tree` lose the page gutter and each level is indented 21px with a `--line` guide (12px on phones) | Owner: "hiển thị giống tree của vscode này, bao nhiêu tầng folder thì bấy nhiêu nhánh, các node thì theo grid". Released (34 assets, build and release green on e0785a1); the mini was redeployed the same day (`docker compose pull && up -d`, container recreated on the image built from e0785a1) and serves 2.8.0 — the served bundle is byte-identical to the local 2.8.0 build except the version string. Checked: `cargo test` (18, incl. new `nests_every_level`), `tsc`, vite build. Not clicked through in a browser. Heads-up: the display lists folders before a folder's own videos at each level (VS Code), while prev/next still follows `tree::entries` (direct videos before subfolders), so the visual order can differ from prev/next inside a mixed folder; the previous/next order itself is unchanged. |
| (2.7.0 commit) | 2.7.0 | library "tree" view (third state of the view button, `GET /libraries/{id}/tree`): folders relative to the library's folder in natural order (`tree.rs`, `util::sort_key` per path part), collapsible, videos listed inside; previous/next on the item page (and so the player) follow the same order for every library (a flat library gives the old order) instead of `sort_title` | Owner's FreeCAD course: 10 chapter folders in one library, 119 videos mixed A–Z. Checked: `cargo test` (17, incl. tree order), `tsc`. Not clicked through in a browser. The order is computed per item request from every id/path of the library (fine for hundreds of items, not cached). "Similar videos" (name-based, other libraries) is unchanged. |
| (2.6.0 commit) | 2.6.0 | audio lessons: scanner indexes audio files (`util::AUDIO_EXT`) and, with Settings → Narration → "Subtitle files without a video or audio file are lessons" (`narration.lessons`, off by default so `Subs/` folders do not turn into items), one `.srt` per `(folder, name)` with no video/audio of that name (English preferred; `util::split_lang` strips `.en`) as a placeholder item (`ext = srt`); built-in pipeline `subs-edge` (subtitle en → translate vi → edge-tts); a job on a `.srt` / audio item is `audio_only`: the worker joins cues into sentences (`sentences()`), translates, `tts_edge(sequential=True)` reads them one after the other (0.45 s gaps, no timeline) into `<lang>.podcast.mp3` (mono 80k), the server (`podcast()`) writes `<name>.mp3` + `<name>.<lang>.srt` beside it and re-points the item at the mp3 (the next scan sees mp3 + srt as one audio item with subtitles); Whisper/OCR pipelines skip such items in `queue`; client: `isAudio`/`isLesson`, audio plays direct on a pure black `.player.audio` with one dim logo, the Item page hides Play for a lesson | Owner's FreeCAD course: several lessons had only `.en.srt`. Checked: `cargo test` in Docker (16 pass, incl. `walk` on a temp folder), `tsc`, `sentences()` on sample cues. NOT run end to end: no worker/edge-tts/Argos here, so the real sub → mp3 → playback path and the black screen on a phone are unverified. Only the first voice of a graph is read in an audio lesson; VieNeu is refused there. |
| `5bbf542` | 2.5.4 | worker: `ocr()` reports (and so learns of a delete or pause, 410 → `JobCancelled`) every 8 s of wall time instead of every 120 frames, and kills its ffmpeg decoder when it stops; `whisper()` waits for the service in 15 s steps and drops the request when the job is gone; `translate()` drops the lines not yet sent | Owner: jobs paused and deleted in Settings, but EasyOCR kept working (htop: `easyocr_server.py` ~370 % CPU). On the mini a read takes ~6 s and the report came every 120 frames, i.e. after ~12 minutes; the two jobs waiting for the OCR lock had stopped within 20 s, only the one in OCR had not (worker log). Restarted the worker by hand to stop it (queue was empty). Checked with a real clip and a slow fake service: OCR stops in <= 8 s and leaves no ffmpeg; Whisper wait is abandoned at the next 15 s step. A Whisper run already started goes on inside its service (no way to cancel it there). Released; the mini runs 2.5.4. |
| `57a3c31` | 2.5.3 | `tts_edge`: one process-wide limit of `EDGE_PARALLEL` (12) lines in flight (`EDGE_SLOTS`, over all videos and both voices), and after the first pass the lines that failed are made again in up to `EDGE_TTS_RETRIES` (3) rounds, 4 at a time, after 10 / 30 / 90 s; only the lines still failing then count towards the 10 % that fails the job | Owner: Smallville Tập 1 failed with "edge-tts failed on 46 of 335 lines" (the Vietnamese voice). Since 2.5.0 three videos x two edge voices x 12 lines were in flight at once (~72 connections to Microsoft), which is the likely cause (not proven: the log only has the count). Checked with a fake `edge_tts` (12 lines failing 4 times recovered in round 1, 3 dead lines tried through all rounds then skipped, two voices at once peaked at 12 connections); not against the real service. A job that already failed needs Retry. Released (34 assets); the mini runs 2.5.3. Same release, at the owner's request ("always redo from the start, even when it exists"): the worker no longer reuses subtitles found beside the video (only an explicit **subtitle** node reads them) and always uploads the ones it made; the server replaces `<stem>.<lang>.srt` (copy to a hidden name, rename over: works on files it cannot write, as the owner's old ones are 0744/0600) instead of skipping existing ones; queueing a finished or failed video queues it again (`priority` reset) — waiting, running and paused ones are left alone, so "Narrate a library" redoes every finished video of it. |
| `f927a60` | 2.5.2 | `components/Toast.tsx` (`notify(text, ok)`, mounted in `App.tsx`) replaces `alert()` for the Narration buttons on item and library pages; the Narration tab shows the action's error under the "Narrate a library" button and above the queue (it was only under the Save button at the top of the page); "0 queued" says why; the worker adds "· subtitles reused: en, vi, zh" to the voice stage's step and logs each reuse | Owner: when queueing was refused nothing seemed to happen, and three Smallville episodes ran OCR and translation "in seconds". The error was there but out of sight (act.Note under Save at the top, `alert()` that app webviews can swallow). The episodes had `.en/.vi/.zh.srt` + `.ocr_done` beside them from the owner's own September `auto_sub.py` run, so the worker reused them as designed: no OCR, no translation, only the voices. Checked on the mini's folder listing and the worker's own `process()` with fake services (no OCR call, labels as above); tsc and a vite build; not clicked through in a browser. Other `alert()`s (AI upscale, image upload, update banner) are still alerts. |
| `95e1311` | 2.5.1 | `checkNarration(graph)` asks only for the services the pipeline uses (Whisper / EasyOCR / Argos / VieNeu / edge-tts by node type; a retry only needs a worker online) and `pickPreset()` returns the preset, not its id; built-in presets "Whisper · edge-tts only (no VieNeu)" and "OCR · edge-tts only (no VieNeu)" (`whisper-edge`, `ocr-edge`); `docker-compose.cpu.yml` has no VieNeu any more (and the `VIENEU_SINGLE` nginx switch is gone); the worker's VieNeu probe first asks `/tts/` with an 8 s timeout | Owner on the mini PC: could not queue narration. The mini's compose (`~/Software/kellyfin/docker-compose.yml`, the film-sub services next to Kellyfin) has VieNeu commented out on purpose ("I do not want VieNeu on the CPU"), the worker reported it as down, and the old check wanted every service up. Seen in nginx's log as a 499 on `/gradio_api/call/_srt_voices` (the probe hung 60 s each time). The mini is reachable as `ssh mini-wg` (user `mini`, port 2408, key). Checked by `cargo test` (every built-in valid), tsc and the service names the worker reports; not clicked through in a browser. Released (34 assets); the mini (server + worker) updated by `ssh mini-wg`, where the worker started 3 videos at once. |
| `0ddb06f` | 2.5.0 | worker: up to `WORKER_JOBS` (3) videos at once with one-at-a-time locks on whisper / OCR / translate / VieNeu, the voices of a video read aloud in parallel, a stop on 410; server: queue **Pause** (waiting or running) / **Resume** / **Top**, "Pause waiting" / "Resume paused", jobs listed in running order, `done`/`progress` answer 410 for a job that is gone; migration `0008_narration_queue` (`priority`) | Owner (all on the mini PC, CPU): edge-tts is slow and the queue had no way to pause or put an episode first. edge-tts already read 12 lines at once inside one video, so the gain is across videos. Checked with the real `process()` and fake services with delays (3 videos: OCR one after another, the next video's OCR while the previous is read aloud, 10 s against ~20 s; a job deleted while waiting stopped without a fail report and left no temp files), the SQL on a database built from the migrations, `cargo test`, tsc and a vite build. Not run against a real queue, and the buttons were not looked at in a browser. Released (34 assets). On a CPU the three overlapping jobs share the cores (OCR + VieNeu + Argos together); lower `WORKER_JOBS` if the mini PC chokes. A paused running job loses its work (OCR/Whisper again on resume). |
| `1277872` | 2.4.4 | `narrator/docker-compose.cpu.yml`: the whole film-sub stack with no GPU (Whisper int8 on the CPU, EasyOCR `EASYOCR_USE_GPU=0`, one VieNeu with `VIENEU_DEVICE=CPU` and a volume for its model, nginx with `VIENEU_SINGLE=1` dropping the second instance); the worker reads `WHISPER_MODEL` (was a fixed `medium`) and `WHISPER_TIMEOUT` (was 3600 s, now 4 h; nginx's Whisper timeout 4 h too); README section "CPU only" | Owner: the graphics card is broken. The existing images already fall back to the CPU (whisper picks it when `nvidia-smi` is missing; EasyOCR has the env; VieNeu v3 Turbo runs through ONNX on the CPU), so no new image. Run for real on koi-pc (12 threads) under another project name: all services answer through nginx (whisper `device: cpu`, ocr `gpu: false`, 23 voices), Whisper medium 0.25x, small 0.09x of the speech length (setting `OMP_NUM_THREADS` made it slower), VieNeu 41 s of voice in 18 s, 2.2 GiB, EasyOCR gave the same 7 cues as on the GPU in 32 s for a 40 s video. VieNeu's model is not in the image: it downloads from Hugging Face on first start. Numbers come from short samples, not a whole film or a real job queue. The GPU containers on koi-pc were left as they were; `docker compose -f docker-compose.cpu.yml up -d` in `narrator/` would replace them (same project and container names). |
| `5bcf1ba` | 2.4.3 | mux: new tracks are Opus 96k in a `.webm` (AAC 128k elsewhere; WebM refuses AAC and the job failed in ffmpeg); a video with no audio track gets the narration alone (no ducking, voices padded, `-shortest` ends the file with the video) instead of the error "the video has no audio"; `FolderPicker` (Add library) has an accent-insensitive filter, reset when you enter a folder | Owner's `sect3-part1.webm` failed at the mux with ffmpeg's WebM codec error. Checked with real ffmpeg runs of the exact filter graphs (vp9+opus webm; silent mp4 gave 6.000 s with an AAC track); `/narrator/jobs/{id}/audio` still answers 422 for a silent video, which only matters for Whisper pipelines (OCR ones do not ask for it). Folder picker checked by build and types only, not in a browser. |
| `3d45dc5` | 2.4.2 | OCR: text behind the picture no longer becomes subtitles (a lecture's slides, a status bar, a file list): lines still for ≥ 10 s are page text, caption blocks not centred are dropped, scraps of page text next to the caption box are cut from it; the band is read at the video's own width up to 1920 px (was 960); a short caption over a still picture now triggers a read (any 48 × 8 px cell changing by 12+, not only the mean) | Owner: screen-recorded FreeCAD/Udemy course (grey caption box over documentation, file lists, slides) gave the page text as the subtitles, and a ~14 px caption in a 2000 px frame was unreadable at 960. Checked on generated videos of those layouts (7 cues of 7 each for the Explorer layout, 6 of 6 for the slide layout, film-like moving picture identical to the old code); the 4 screenshots the owner sent were not available as files, only rebuilt from their layout. Not re-run on a real hard-subbed film (none on the GPU PC). Released (34 assets); the GPU PC has the new narrator image pulled (stack was stopped), the mini was not updated from the session (no SSH key) and does not need it for this change. A caption on screen ≥ 10 s counts as page text; slide text that changes more often than every 10 s near the centre can still leak; page text right beside the caption on its own row can leave a scrap. |
| `2c8acf1` | 2.4.1 | worker: a failed edge-tts save deletes its partial mp3, a line whose mp3 does not decode is made again once and then left silent (job fails past 10 % of lines); OCR node: languages as tags with a filterable "+ Add language" (first tag = the subtitles' language, same `lang` + `also` data), a new OCR node has none and does not save until one is chosen; queue: "Clear finished" / "Clear failed" (`POST /api/admin/narration/jobs/clear`) | A Promised Neverland episode failed in TTS: ffmpeg could not read a truncated edge-tts mp3 and the whole job stopped. The other two are the items noted for this round. Released (34 assets); server and worker updated. |
| `7d8f71c` | 2.4.0 | every `<select>` is `components/Select.tsx` (button + fixed pop-up with an accent-insensitive filter, keyboard, hidden input for forms); pipeline canvas full screen (Esc closes); OCR node: subtitle language + "also read" languages, EasyOCR's only (vi, en, zh, ja, ko) — the worker reads each frame once per language and keeps the surer box; workers report edge-tts voice names, voice nodes list the ones for their input language; the worker runs a pipeline lazily (translations and unwired sources are outputs, a translation the video already has does not run its source, so Whisper/OCR are skipped when everything exists) | Owner: filter on every drop-down, and the items noted for this round. The EasyOCR service takes one language per request ("vi,en" → 500). Released (34 assets); server and worker updated. OCR "also read" joins every language into one line in the main language: for bilingual burned-in subtitles use one OCR node per language instead. |
| `48911bb` | 2.3.0 | narration pipelines: `pipeline.rs` (graph types, validation, built-ins "Default · Whisper" / "Default · OCR"), migration `0007_narration_pipelines` (presets table, `preset`/`pipeline` on jobs), presets API, retry keeps the job's graph, `/narrator/jobs/{id}/video` for OCR; worker runs the graph (OCR of burned-in subtitles, edge-tts for any language, voices in any language); `PipelineEditor` canvas (drag nodes, wire ports, click a wire to drop it); "Narration" buttons ask for a pipeline | The owner asked for OCR + Whisper defaults and a free node canvas, saved as named presets. OCR checked on 3 min of Bát Tiên: 17 lines in 9 s, text and timing matching the hand-made `.vi.srt` (one short line missed). Canvas checked in Chromium with a mocked API. Released (v2.3.0 34); server (migration 0007 applied) and worker updated. |
| `9fd784f` | 2.2.11 | worker `dedupe()`: a Whisper line more than 60 % like (difflib, letters only) one of the last 3 kept lines within 10 s is dropped and the kept line stays on screen until the dropped one ends | The owner's rule from earlier hand-made runs: Whisper repeats short lines 2–3 times with a character changed. Repeats more than 10 s apart are kept (real dialogue). |
| `3351f69` | 2.2.10 | the Vietnamese voice is a `<select>` of the voices the workers report (a text field when no worker is online); worker hint names `narrator/docker-compose.yml` | The `<datalist>` only suggested entries matching the text already in the field, so the 23 voices never showed. Released (v2.2.10 34). |
| `a80f342` | 2.2.9 | narration results go into the media folders: audio tracks muxed into the video file, `.srt` beside it (see §9); the virtual tracks (indexes 1001/1002, `n<lang>` subtitles, second inputs in remux/HLS) are gone; media mounts writable | Owner: "nhét thẳng vào folder phim … nhúng mux vào thẳng các phim gốc giống Energon, luôn luôn như vậy". Dry-run on a copy of a 3Blue1Brown video: 4 streams (av1, und, vie, zho), same duration. Deployed: at start-up the server embedded the 9 finished 3Blue1Brown narrations (each mp4 now `und|eng, vie, zho`, `.en/.vi/.zh.srt` beside it, no temp files left). Release has 33 assets: the Windows x86 desktop job failed on "Not Found" while tauri-action updated a release asset (upload race, not a build error); re-run it in the Actions UI (the PAT cannot). |
| `0b0f197` | 2.2.8 | the worker leaves lines of punctuation only silent (Argos turned one into ".") and skips a line edge-tts still refuses after 3 tries, failing the video only past 10 % of its lines | "edge-tts failed on: ." failed a whole 3Blue1Brown video. Released. First jobs through the server-side mux: job 9 (3Blue1Brown) uploaded its voices (7 + 8 MB FLAC) and the mini PC muxed both in 22 s while the worker took the next video. |
| `88d8c11` | 2.2.7 | `0006_narration.sql` restored byte for byte to its 2.2.0 content; `build.yml` fails when a migration differs from the first release that shipped it | **Never edit a released migration**, not even a comment: sqlx checksums the file. Change the schema with a new migration. Released; the mini PC is back on `:latest` (2.2.7) and the worker runs 2.2.7. At the owner's request every 3Blue1Brown narration (9 jobs, 7 result folders) was deleted to start over. |
| `73ec96c` | 2.2.6 | narration stage 4 (mux) moved from the worker to the server (`muxing` status, resumed at start-up); the worker uploads the voice as FLAC and goes on to the next video | Owner's request: keep the GPU machine busy with GPU steps only. The worker and server must be updated together (the server no longer accepts `<lang>.m4a` uploads). **Broke the server**: it also edited a comment in the already-applied `0006_narration.sql`, so sqlx refused to start ("migration 6 was previously applied but has been modified"); the mini PC was rolled back to `:2.2.3`. |
| `063d84b` | 2.2.5 | progress reports keep a busy worker listed; a worker that asks for work gets its own "running" jobs back in the queue; worker retries Kellyfin calls for ~1 min; a Whisper health timeout counts as "busy" | Updating the worker container cut off a running job, which stayed "running" until the 15-min stale timeout ("2 processing" with one worker) and kept Whisper busy, so the next health probe timed out (✗ while transcription worked). A busy worker stopped polling and fell off "Workers online" after 2 min. Worker names must be unique. |
| `e444e24` | 2.2.4 | "Narration" button on a library page (admins): asks the spoken language, checks the workers' report, queues every video | Owner's request. |
| `5ef6c37` | 2.2.3 | the worker's edge-tts check reads its short stream to the end | Returning on the first audio chunk left aiohttp sessions open ("Unclosed client session" every 5 min in the log). Released; server and worker updated. |
| `e3e0fff` | 2.2.2 | worker sends service checks + VieNeu voices with every claim; Settings shows them per worker; the film-sub URL setting and `/api/admin/narration/check` are gone; the voice form is filled once (it was reset by the 4 s refresh); `narrator/.env.example` restored | The owner asked why Kellyfin needs the film-sub address when the worker connects by itself — it did not. The handoff commit `a2d6035` had dropped `.env.example` with `git commit -a` while the file was briefly gone. Released; worker on koi-pc reports all four services OK. |
| `346a51c` | 2.2.1 | Settings → Narration: "Check server" saves the typed URL, shows "Checking…" and always reports; worker snippet = `.env` lines for `narrator/film-sub`; player menu rule `.check { width: 18px }` scoped to `.p-menu-item` | Check was disabled until a URL had been saved, so clicking it did nothing. The global `.check` rule squeezed every `label.check` (narration languages, Administrator, library options) to 18 px, so their texts overlapped. Released. |
| `a6c4009` | 2.2.0 | narration: `narration.rs` + migration `0006_narration`, worker `narrator/` (image `hienanh/kellyfin-narrator`) as service `kellyfin-narrator` of the film-sub compose, Settings → Narration (URL, voices, server check, queue with stages), "Narration" on a video's page; remux/HLS take a narration track as a second input | See §9. The owner chose: never touch the video files; the worker lives in the film-sub compose; Chinese voice = edge-tts. Released (34 assets); `kellyfin-narrator` runs in film-sub on koi-pc and checks in with the mini PC. |
| `917a7f6` | 2.1.3 | HLS transcode pads audio with `apad` + `-shortest` + `-max_interleave_delta 0` | Energon E05–E08: the Vietnamese dub (audio 2) ends ~55 s before the video (E06: 1227 s vs 1282.8 s), so segments after it had no audio packets and the player stalled ~40 s before the end, walking the quality down to 144p. `apad` alone emits all the silence at EOF into the last segment; `-max_interleave_delta 0` makes it follow the video. Verified by counting audio packets per segment on the mini PC. Released (34 assets). |
| `2bf3628` | 2.1.2 | "Auto-play next" switch in the player's settings menu (`localStorage.autoNext`); a watched video resumes where it was left and is listed in Continue Watching (the ✓ stays; finishing resets the position); `dev` branch removed, CI and the mini PC use `main` / `:latest` | Before, `played` made the player start at 0 and hid the item from Continue even though the server kept saving its position. Released (34 assets); the mini PC now pulls `:latest`. |
| `1dc437b` | 2.1.1 | background playback: Android `MainActivity.onPause` resumes the WebView again while the player says it is playing (`KellyfinNative.setPlaying`); iOS Info.plist gets `UIBackgroundModes: audio` (PlistBuddy step in `mobile.yml`) | `WryActivity.onPause()` calls `WebView.onPause()`, which stops media. Not verified on devices yet; iOS WebKit may still pause a `<video>` (not an `<audio>`) when the screen locks, in which case PiP is the fallback. The music feature was dropped by the owner. Released (34 assets). |
| `664e803` | 2.1.0 | `restore()` gives up on `/me` after 8 s and Login says the server is unreachable (pick another, e.g. the VPN one); seek bar drawn by hand with a 28 px hit area, thicker + thumb on hover; Alt+P / Option+P = picture-in-picture (`e.code`, a Mac types π); controls fade 1 s after the mouse leaves and stay while hovered | A LAN server opened from outside hung on the splash forever (no fetch timeout). The invisible range input over the seek bar mapped clicks with its thumb inset, so the fill stopped tens of px short in WebKitGTK. Touch auto-hide stays at 3 s (the owner asked for 5 s unless already lower). |
| `f4876dc` | 2.0.12 | Linux app fullscreen = Tauri window fullscreen (`core:window:allow-set-fullscreen`), Esc leaves it; volume bar is plain divs with pointer capture; F5 / Ctrl+R / Cmd+R and "Reload" in the avatar menu | `requestFullscreen` made WebKitGTK 2.50 abort the UI process (SIGABRT inside libwebkit2gtk, from `/var/crash/_usr_bin_kellyfin.1000.crash`). A styled `<input type=range>` still renders differently in WebKitGTK (thick track, thumb short of the end). Released (34 assets). |
| `d8e0c07` | 2.0.11 | `appleWebKit` needs a Mac/iOS UA, so WebKitGTK (Linux app) gets direct play / remux | Only the Linux app lagged on Athena: its UA has `AppleWebKit` without `Chrome`, so the Apple rules (no remux, no hev1) sent every HEVC file to a 1080p VAAPI transcode that the mini PC's CPU HEVC decode can barely keep at real time. GStreamer on koi-pc decodes the same stream at ~600 fps (`avdec_h265`). Released (34 assets). |
| `32ce4b0` | 2.0.10 | transcode jobs keyed by job, up to 3 per item (least recently used dropped); volume slider drawn in CSS; desktop icons regenerated from `app-icon.svg` with a 6% margin like kermius (Android/iOS icons unchanged) | Two devices on one episode cancelled each other's ffmpeg ("superseded") every few seconds: playback crawled at ~0.5 fps and the stall handler walked the quality down to 144p. |
| `de1674d` | 2.0.9 | previous-episode button; previous/next sit on both sides of the clock; the centre play/±10 s overlay is shown with a mouse too (bottom play/±10 s removed) | Owner's request. Released (34 assets). |
| `3509152` | 2.0.8 | a `waiting` event in the last 2 s counts as the end (up next starts) | The variant playlist sizes the last segment from the container duration (E01: 2.821 s) but the encode holds 2.767 s; iOS's native HLS waits for the missing 54 ms forever. |
| `255268e` | 2.0.7 | crop zoom re-measured on `resize`; icon buttons `flex: none` and hover only on `(hover: hover)` | On iOS HLS `videoWidth` is 0 at `loadedmetadata`, so the zoom stayed off until something else resized the page. A long title shrank the top-row buttons into ovals; iOS keeps `:hover` after a tap. Released (34 assets). |
| `b9d4045` | 2.0.6 | black-bar detection (`cropdetect`, migration `0005_crop`), player zoom past the bars with an off switch, cropped thumbnails | Checked on Energon: 1280×720 files with 160 px pillarbox → `960:720:160:0`. Rescan everything once to fill `crop`. Released (34 assets). |
| `ba3d65e` | 2.0.5 | a replaced file (size/mtime changed) gets a new frame-grab thumbnail unless its image is custom; Scan in Settings → Libraries shows progress until the scan ends and then reloads rows; `text-size-adjust: 100%` | A per-library Scan used to leave "Scanning X" on screen forever and only re-probed changed files, so cropped re-encodes kept their old black-bar thumbnails. iOS inflates text in landscape unless `-webkit-text-size-adjust` is set. |
| `ff35e24` | 2.0.4 | remove brightness/volume swipes (and their Android/iOS native code), disable page zoom, "10" inside the skip icons | A double tap zoomed the iOS page and the swipe handlers then ate the pinch back. Released (34 assets). |
| `95dfe2c` | 2.0.3 | pull to refresh: decide vertical vs sideways only after 10px | The first `touchmove` often moves 1–2px sideways, which cancelled the pull. 2.0.2's release run could not be cancelled with the PAT (no Actions scope). Released (34 assets). |
| `ce2ea71` | 2.0.2 | iOS: blank Home / "?" thumbnails after the app was in the background; pull to refresh; Home error + Retry | iOS reclaims a suspended app's listening sockets: check the local proxy on resume and rebind. `alive_until_stopped` test covers the check. |
| `2cdef6b` | 1.5.0 | device brightness/volume/landscape fullscreen in the apps, centre player controls, top-bar safe-area fix, one view button | A fixed-height bar with safe-area padding squeezes its content; CSS rules declared earlier lose to later ones of equal specificity (twice). |
| `8254c7f` | 1.4.0 | Jellyfin-style More menu instead of the sidebar, ← button, Android back + double-back exit, grid/list view | Measuring which chips fit must not depend on a button that the result itself shows or hides (it clipped the last chip). Released (20 assets, CI green); A8 Star updated 1.2.0 → 1.4.0. |
| `80c47a4` | 1.3.0 | library sidebar (☰ on PC, logo on phones), wheel scrolls the chip bar | Hidden scrollbars leave mouse users stuck: map vertical wheel to horizontal scroll. |
| `78e547f` | 1.2.0 | iOS/macOS/Linux apps reach plain-http servers via the local proxy; app and server versions shown | The iOS failure was WebKit mixed-content blocking from `tauri://`, not ATS. Released (20 assets, all CI green); the A8 Star updated 1.1.0 → 1.2.0; not yet verified on a real iPhone. |
| `a922f0b` | 1.1.0 | keyboard no longer covers inputs, Android http servers, multi-server switch on web and apps | v1.0.0's APK had `usesCleartextTraffic=false` (Gradle placeholder): verify the built APK, not the patch script. Released; the 1.1.0 APK has cleartext on and updated the A8 Star in place over 1.0.0. |
| (docs) | | README rewritten for GitHub visitors, 12 screenshots | Retake shots by driving the live server with Playwright (desktop 1440×900, Galaxy S9+ portrait/landscape); Escape in the player goes back, so do not press it between shots. |
| `15f74c2` | 1.0.0 | first release: `dev` → `main` | First `release.yml` run: every job green; 20 assets (deb/rpm/AppImage x64+arm64, dmg/app x64+arm64, NSIS/MSI/portable x64+x86+arm64, APK, unsigned IPA). The PAT cannot list Actions secrets, so the APK keystore secrets were not verified. |

## 8. P7 — AI upscale (experimental pipeline)

Built on 2026-10-20 and verified end to end with the stub model (Lucky Luke trailer 854×468 → 1970×1080 with NVENC
on the RTX 3060 in 13 s, uploaded over WireGuard, played as "AI · lanczos").

- **Pull model**: workers only make outgoing requests, so the GPU PC needs no open port. `POST /api/upscaler/claim`
  (header `X-Upscaler-Token`) announces the worker and its models and atomically takes the oldest queued job for one
  of them (running jobs silent for 10 min are reassigned); then `GET …/jobs/{id}/source`, `POST …/progress`,
  `PUT …/result` (streamed to `data/upscaled/<item>-<model>.mp4`, probed, job → done) or `POST …/fail`.
- **Assignment** = a job `(item, model)`: Settings → AI upscale queues a whole library, the video page's
  "AI upscale" button one episode; failed jobs are retried by queuing again. Workers online are kept in memory.
- **Playback**: finished versions are listed in `ItemDetail.upscaled`; the player's quality menu offers
  "AI · model (1080p)", which plays `/api/items/{id}/upscaled/{model}` directly.
- **Worker** (`upscaler/kellyfin_upscaler.py`, stdlib + ffmpeg; image `hienanh/kellyfin-upscaler` from CI; compose
  with the NVIDIA GPU): models `lanczos` (stub), `realesrgan-anime` when `realesrgan-ncnn-vulkan` is on PATH, and
  any `models/<name>.sh` (`INPUT OUTPUT TARGET_HEIGHT` → MP4) — that is where trained models go.
- Not done: live (per-HLS-segment) upscaling; per-library default model; worker not running permanently anywhere.

## 9. Narration (thuyết minh)

Turns a video into one with a Vietnamese and/or Chinese voice-over, the way the owner did by hand for Transformers:
Energon (method notes: `/mnt/das_8tb/metube_volume/NOTE_pipeline_sub_dich_tts_mux.md`, scripts in that folder).

- **Where things run**: `narrator/docker-compose.yml` in the repo is the compose for the GPU side (public `hienanh/*` images;
  their sources are not in this repo). On koi-pc the owner's own copy runs (`/mnt/ssd_1tb/Downloads-Ubuntu/Miscellaneous/film-sub`,
  `http://192.168.0.182:8080`: `/whisper/`, `/translate/` (LibreTranslate/Argos), `/gradio_api/` (two VieNeu-TTS
  instances, sticky cookie), `/ocr/`) now also has the `kellyfin-narrator` service (`FILMSUB_URL=http://nginx`,
  token in that folder's `.env`). It only makes outgoing requests, like the AI upscale worker. Kellyfin never
  calls it or the services (since 2.2.2; 2.2.0–2.2.1 had a film-sub URL setting for a server-side check): with each
  claim the worker reports its own checks (Whisper health, Argos languages incl. en/vi/zh, VieNeu voices — about once
  a minute — and edge-tts every five minutes), shown under Workers online; queueing requires a worker whose checks
  all pass.
- **Pipelines** (2.3.0, `server/src/pipeline.rs`, editor `src/components/PipelineEditor.tsx`): a job runs a node graph
  — sources `whisper` (lang/auto), `ocr` (lang, bottom crop share), `subtitle` (the video's own); `translate` (to a
  lang); `voice` (vieneu = Vietnamese only, or edge = any language with a default voice per language); one `mux`.
  Wires: source/translate → translate/voice, voice → mux; checked server-side (types, one input for translate/voice,
  no loops, one voice per language). Built-ins in code, saved presets in `narration_presets`; each job stores its graph
  (`narration_jobs.pipeline`), so editing a preset never changes queued jobs. Jobs from before 2.3.0 run the Whisper
  graph built from their old `source`/`langs`. OCR: the worker downloads the video (`/narrator/jobs/{id}/video`),
  decodes the bottom band at 2 fps (ffmpeg → raw RGB, at the video's width up to 1920 px: a caption is ~14 px tall in a 2000 px frame and cannot be read at 960), compares a 320 px grey copy with the last read frame
  (mean change ≥ 4, or ≥ 12 in any 48 × 8 px cell, or every 5 s → EasyOCR `/ocr/ocr`), keeps every read, then
  drops what is not a subtitle: watermark text; lines that stay in one place ≥ 10 s (page text behind the picture,
  a caption box may cover them up to 20 s, scraps of them left beside the box are cut off the caption); caption
  blocks whose middle is not within 15 % of the centre (boxes of one row that nearly touch are one line; lines one
  under the other that overlap sideways are one block, so a left-aligned two-line caption in its grey box counts by its
  box; if nothing is centred the rule is skipped). Then joins lines 60 % alike, drops lines under 0.7 s, merges equal lines ≤ 2 s apart.
- **Concurrency and queue control** (2.5.0): the worker claims up to `WORKER_JOBS` (3) jobs, one thread each (`run_job`), and sends `active` (its running job ids) and `take` (false = every slot busy, a heartbeat only) with `/narrator/claim`; the server puts back only the worker's running jobs that are not in `active`, and takes over a stale (15 min) running job only from another worker. Heavy services are `RES` locks in the worker (whisper, ocr, translate, vieneu), taken around the call after the download; a job waiting for one reports every 20 s (keeps it alive, lets a deletion through). The voices of one job run in a thread pool. `/narrator/jobs/{id}/progress` and `/done` answer **410** when the job is not running any more (deleted, paused); the worker raises `JobCancelled` and stops without calling `/fail`. Admin: `POST /admin/narration/jobs/pause` and `/resume` (`{ids: []}` = all waiting / all paused; a listed running job is paused too and starts over), `POST /admin/narration/jobs/{id}/top` (`priority` = max + 1, status queued; claim order is `priority DESC, id`; migration 0008). The queue lists waiting jobs in the order they will run.
- **Stages** (`narration_jobs.stage`, shown as chips in the queue): 1 subtitles in the spoken language — the video's
  own text subtitles, else Whisper (`language` = the chosen code or `auto`; with `auto`, Argos `/detect` names the
  language from the text, since the Whisper service does not say; repeated lines are merged, see 2.2.11); 2 translation into en, vi, zh (Argos, 6 parallel
  requests per file; since 2.5.3 nothing is reused: a job redoes every step and replaces the `.srt` files beside the video); 3 voice: VieNeu `wrapper_2` (SRT upload, keep timing,
  wav) for vi, edge-tts per cue placed on a 48 kHz timeline with numpy for zh (needs Internet); 4 mux — on the Kellyfin
  server since 2.2.6: the worker uploads `<lang>.voice.flac`, calls `done` (status `muxing`) and takes the next video,
  while the server mixes the voice over the original track read from the video file itself (`sidechaincompress`
  ducking + `amix normalize=0`, voice `apad`ded, AAC 128k, as long as the original), one job at a time (semaphore),
  deleting the FLAC; jobs still `muxing` are resumed when the server starts. The worker fetches the original sound
  only when Whisper needs it.
- **Results** (since 2.2.9, the owner's wish: "like Energon, always"): the worker's uploads wait in
  `data/narration/<item>/`; the mux then rewrites the video in place — `-map 0:v`, every original audio track except
  an earlier narration in a language made now (matched by language tag: MP4 drops track titles), one new AAC 128k
  track per voice (`language=vie|zho`, title "Narration (…)" where the container keeps it, not default), `-map 0:s?`,
  `-c copy`, `+faststart` for MP4 — into a hidden `.<stem>.narrating.<ext>` in the same folder (the scanner skips dot
  files; same file system for the rename), verified with ffprobe (video present, audio count, new languages, duration
  within 2 s) before the rename; then the item is re-probed (`scan::refresh_item`) and its embedded-subtitle VTT cache
  dropped. Subtitles are copied next to the video as `<stem>.<lang>.srt` unless such a file exists, and the data folder
  is removed. Narrations made by 2.2.0–2.2.8 (separate `<lang>.m4a` in data) are embedded the same way at start-up.
  Media mounts are writable since 2.2.9.
- **Tested** before 2.2.0 on a 60 s Energon clip, in the worker image against film-sub: Whisper `auto` + Argos
  detect (en, 5.6 s), translation vi/zh (~1 s each), VieNeu (7.6 s), edge-tts (3.1 s), mix lengths = original.
  The old mux graph cut the narration at the last spoken cue (`sidechaincompress` ends with its shorter input —
  why Energon's vie tracks are ~55 s short); the voice is now `apad`ded first.
- **Running**: since 2026-11-03 `kellyfin-narrator` runs in film-sub on koi-pc (token in film-sub's `.env`, mode 600);
  the server makes the token the first time Settings → Narration opens (or a worker sends any token).
- **Verified** 2026-11-03: jobs through Kellyfin itself (queue → claim → upload → server mux → done) on 3Blue1Brown.
- **Not done / unverified**: playback of a narration track on every device; OCR of burned-in subtitles (the note's §2b) is not part of it; deleting an item leaves its
  `data/narration` folder behind (same as upscaled files).

