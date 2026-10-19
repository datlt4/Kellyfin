# Kellyfin — handoff

State on 2026-10-20, version **1.0.0** — first release: `dev` was fast-forwarded into `main`, which runs
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
| P7 | AI upscale (GPU server) | not started (§8) |

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
- Kellyfin listens on **:8024** (→ container :8080). Jellyfin keeps running on :8021, untouched.
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
| Jellyfin API key | Jellyfin dashboard → API Keys (only needed to re-run the export) |

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
  src/transcode.rs      on-demand HLS: one ffmpeg job per item, throttling, VAAPI/NVENC/x264
  src/recommend.rs      title tokens, related libraries/videos, next season, search
  src/admin.rs          dashboard, users, libraries, item edits, uploads, restart/shutdown, import/export
  src/manifest.rs       portable export format (manifest.json + images/)
  src/jellyfin.rs       read-only export from a Jellyfin server into that format
  src/settings.rs       key/value settings (transcode.hwaccel, transcode.device)
  Dockerfile            web build + server build + Debian runtime (ffmpeg, mesa VAAPI)
  docker-compose.yml    the mini PC deployment
src/                    SolidJS client
  api.ts                API client, server profiles, auth, formatting helpers
  playback.ts           playability decisions, quality ladder, bandwidth test, VTT parser
  cast.ts               Chromecast sender + AirPlay
  pages/                Home, Library, Item, Player, Settings (5 tabs), Search, Login
  components/           TopBar, Row (shelf), Card, Icon (SVG), FolderPicker
src-tauri/              Tauri shell; src/proxy.rs = local header-adding proxy
  android/apply.sh      patches the generated Android project (cleartext http, gradle repos)
scripts/
  set-version.sh        sets the version in every manifest and lockfile
  build-mobile.sh       android | ios  (local builds, same steps as the mobile workflow)
  build-desktop.sh      linux | linux-arm64 | windows | macos
  desktop.Dockerfile    Ubuntu 22.04 build image (glibc 2.35) with cargo-xwin + NSIS
  publish-release.sh    branch release/vX.Y.Z + tag + GitHub release + upload dist-*/
.github/workflows/      build.yml, release.yml, mobile.yml (see §5)
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

Mobile gestures: vertical swipe left half = brightness (CSS filter 0.2–1.5, remembered), right half = volume
(iOS ignores scripted volume; the HUD says so). Double tap a side = ±10 s.

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

Native apps store several servers `{name, url, headers}` and a session per server. If a server has headers
(e.g. a Pangolin access token), `src-tauri/src/proxy.rs` serves `http://127.0.0.1:<port>/<random secret>/…`
and forwards everything — API, images, video, HLS — adding the headers, streaming both ways with ranges.
Reason: `<video>`/`<img>` cannot send custom headers. Tested by a unit test only; **not yet run inside a real app**.

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

Prepared but **not activated**:
- `~/kellyfin-ios/kellyfin-ios-resign.sh` (from Kermius' script): downloads the newest release IPA, keeps a
  7-day free-account profile for `com.datlt4.kellyfin` via the helper project `~/kellyfin-ios/helper`
  (Kermius' helper with the bundle id changed), re-signs, installs, prints the dashboard markers.
- `~/kellyfin-ios/com.datlt4.kellyfin-resign.plist` (every 6 h) — copy to `~/Library/LaunchAgents` and
  `launchctl load` **after** the first release with an IPA exists.

To do after the first release: `~/ipa-resign/jobs/kellyfin.json` (script mode, `managed: false`,
`forceArg --force`, `renewArg --renew`, artifact `github-release datlt4/Kellyfin \.ipa$`,
`install.package com.datlt4.kellyfin`) and `~/ipa-resign/jobs/kellyfin-android.json` (artifact mode,
`\.apk$`, `install.package com.koi.kellyfin`, `managed: true`, `intervalMinutes 720`, `autoUpdateOnConnect` —
the A8 Star is the Samsung G885F). A free Apple ID allows only 3 sideloaded apps per device; Kermius is already one.

---

## 6. Open items

1. **Run the repair on the server** (Dashboard → Check → Fix): 6 wrong Jumong items; review the 39 others.
2. **Finish P5**: v1.0.0 is out; confirm repo secrets `ANDROID_KEYSTORE_B64/PASS`
   (keystore in `~/.config/kellyfin/`) — without them the APK is signed with a throwaway key; then activate the two
   re-sign jobs on mabu and install on the iPhone and the A8 Star.
3. **Linux arm64 desktop** is covered by CI (`ubuntu-22.04-arm`); locally it would need QEMU binfmt.
4. **Verify on real devices**: hev1/remux rules on iOS, header proxy in the apps, Android WebView fullscreen
   and PiP (may need a native plugin), Chromecast/AirPlay.
5. Disk on koi-pc: Rust/Docker build outputs were deleted on request (~19 GB). `server/target`,
   `src-tauri/target` and the `rust:1-bookworm` image come back whenever tests run locally — delete them after.
   Docker's build cache (~75 GB, mostly other projects) was left alone.
6. Android/iOS apps have no in-app updater (Kermius uses the Tauri updater with signing keys; not set up here).
7. The owner may want to retire Jellyfin once satisfied; Kellyfin never writes to media folders.
8. P7 (§8).

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
| `15f74c2` | 1.0.0 | first release: `dev` → `main` | First `release.yml` run: every job green; 20 assets (deb/rpm/AppImage x64+arm64, dmg/app x64+arm64, NSIS/MSI/portable x64+x86+arm64, APK, unsigned IPA). The PAT cannot list Actions secrets, so the APK keystore secrets were not verified. |

## 8. P7 — AI upscale (planned, nothing built)

Owner's idea: some shows are only available at 144p; a model trained per show would upscale to FHD on the
RTX 3060 PC. Planned shape: a separate GPU service (`kellyfin-upscaler`, HTTP job API) on koi-pc; a table
assigning a model to libraries/episodes; first offline pre-upscale into an extra "1080p AI" version of the file,
later live upscaling of HLS segments. Start with a stub model (lanczos or a pretrained Real-ESRGAN) to build the
pipeline before any training.
