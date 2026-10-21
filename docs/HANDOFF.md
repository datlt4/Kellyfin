# Kellyfin — handoff

State on 2026-10-20, version **2.0.12** (2.0.0: in-app updates, AI upscaling, Jellyfin retired; 2.0.1: iOS settings fixes; 2.0.2: iOS proxy restart after suspension, pull to refresh; 2.0.3: pull gesture no longer cancelled by first-touch jitter; 2.0.4: player swipes removed, no page zoom, "10" in skip icons; 2.0.5: rescans refresh thumbnails of replaced files, scan progress in Libraries, no iOS landscape text inflation; 2.0.6: black bars detected by the scan and cropped in the player and thumbnails; 2.0.7: crop applied as soon as the video size is known, round icon buttons on phones; 2.0.8: no endless spinner at the end of transcoded videos (its release run was cancelled; shipped in 2.0.9); 2.0.9: previous/next around the clock, centre play/±10 s on every screen; 2.0.10: several transcodes per video, hand-drawn volume slider, desktop icon with a margin; 2.0.11: the Linux app plays HEVC itself instead of a transcode; 2.0.12: Linux fullscreen without the WebKitGTK crash, hand-drawn volume bar, reload in the desktop apps). 1.0.0 was the first release: `dev` was fast-forwarded into `main`, which runs
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

- Compose project: `/home/mini/Software/kellyfin/` (`docker-compose.yml` + `.env` with `KELLYFIN_TAG=dev`).
- Data (SQLite DB, images, transcode scratch): `/mnt/das_8tb/kellyfin_volume/data`.
- Jellyfin export used for the migration: `/mnt/das_8tb/kellyfin_volume/jellyfin-export/`.
- Kellyfin listens on **:8024** (→ container :8080). Jellyfin was retired on 2026-10-20: container, image,
  `config/` and `cache/` removed; `/home/mini/Software/jellyfin/docker-compose.yml` kept; the videos and subtitles in
  `jellyfin_volume/{media,movies,hanzii}` and `metube_volume` are untouched (Kellyfin uses them). The Jellyfin
  export in `kellyfin_volume/jellyfin-export/` is the only copy of its metadata.
- Media is mounted **read-only** at the same paths Jellyfin uses: `/media`, `/movies`, `/hanzii`, `/metube_volume`.
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
`tauri.conf.json`. Images: Docker Hub `hienanh/kellyfin` (`:dev` + `:X.Y.Z-dev` from dev, `:latest` + `:X.Y.Z` from main).

### CI is the build system

GitHub Actions builds everything (it was briefly disabled during an Actions outage on 2026-10-19 and is back on):
- `build.yml` — every push to `dev`/`main`: web build, server tests, Tauri tests, multi-arch image
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
docker build -f server/Dockerfile -t hienanh/kellyfin:dev .
docker save hienanh/kellyfin:dev | gzip -1 | ssh mini-wg 'gunzip | docker load && cd ~/Software/kellyfin && docker compose up -d'

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
| (next) | 2.0.12 | Linux app fullscreen = Tauri window fullscreen (`core:window:allow-set-fullscreen`), Esc leaves it; volume bar is plain divs with pointer capture; F5 / Ctrl+R / Cmd+R and "Reload" in the avatar menu | `requestFullscreen` made WebKitGTK 2.50 abort the UI process (SIGABRT inside libwebkit2gtk, from `/var/crash/_usr_bin_kellyfin.1000.crash`). A styled `<input type=range>` still renders differently in WebKitGTK (thick track, thumb short of the end). |
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
