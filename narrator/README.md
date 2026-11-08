# Narration worker (thuyết minh)

Gives videos a Vietnamese and/or Chinese voice-over — muxed into the video file as extra language-tagged audio tracks
(the video is copied, not re-encoded) — plus subtitles in the spoken language, English, Vietnamese and Chinese next to
the video (`<video>.<lang>.srt`), like the hand-made Transformers: Energon episodes. Kellyfin's media folders must be
mounted writable.

Each job runs a **pipeline**, a node graph edited in **Settings → Narration → Pipelines** (built-in: "Default ·
Whisper" and "Default · OCR"; duplicate one to make your own), in four stages shown in the queue:

1. **Subtitles**: [Whisper](https://github.com/SYSTRAN/faster-whisper) on the speech (language set, or detected by
   Argos; lines repeated 60 % alike are merged), **OCR** of subtitles burned into the picture (EasyOCR on the bottom
   part of the frame, twice a second, read again only when that band changes), or the video's own subtitles;
2. **Translation** with Argos / LibreTranslate into the languages the graph asks for. A job always redoes every
   step: subtitles, translation and voices of an earlier run are replaced (the `<video>.<lang>.srt` files and the
   narration tracks), only a **subtitle** node reads the video's own subtitles;
3. **Voice**: VieNeu-TTS reads Vietnamese, edge-tts any language (edge-tts is a Microsoft service: needs Internet; a line it cannot make — Microsoft throttles or drops connections — is read by **Google Translate TTS** instead, and a job whose probe fails goes wholly to Google; 20 s timeout per edge-tts line);
4. **Mux**, on the Kellyfin server: the voice over the original sound, which ducks while the voice speaks. The worker
   hands the voice over and takes the next video meanwhile.

## Audio lessons (subtitles only, no video)

Courses often come with `.en.srt` files for lessons whose video did not download. Turn on **Settings → Narration →
"Subtitle files without a video or audio file are lessons"**, scan, and each such `.srt` is listed as a lesson. Queue
the **"English subtitles → Vietnamese voice (edge-tts) · audio lessons"** pipeline (or a copy of it: `subtitle` →
`translate` → `voice` with edge-tts) on the library. The worker joins the cues into sentences, translates them, reads
them one after the other and Kellyfin saves `<name>.mp3` and `<name>.vi.srt` next to the subtitles; the lesson then plays
like a podcast on a black screen with a dim logo, subtitles included. Videos in the same library that have an English
subtitle file get the voice muxed in as before. Whisper and OCR pipelines skip lessons. Needs edge-tts (Internet).

The Whisper image's own `server.py` runs a transcription inside its event loop, so while it works the service answers
nothing (not `/health`) and a worker's check times out; `whisper/server.py` here is that file with the transcription in a
thread, mounted over it by the compose files. Without the mount everything still works, but a deleted job's run can make
the next job wait (the worker waits for it instead of failing).

## Several videos at once

By default the worker works on **one video at a time** (sequential) — edge-tts throttles a flood of connections from one
address (intermittent `NoAudioReceived`, 5–9 s per line), so three at once makes every one of them crawl. To go back to
several at once, Settings → Narration → **Videos at once: Parallel (3 at a time)** (the worker never exceeds `WORKER_JOBS`,
default 3). Whatever the setting, OCR, Whisper, translation and VieNeu each serve one video at a time: a video waits for its
turn ("Waiting for OCR…" in the queue). The voices of one video (say Vietnamese and Chinese) are read aloud at the same
time too.

**Queue** (Settings → Narration): *Pause waiting* / *Resume paused* for all waiting videos, and per video **Top**
(work on this one next), **Pause** (also for a running one: it stops at its next report and starts over when resumed),
**Resume**, **Delete** (a running video is stopped too). To put one episode first: *Pause waiting*, then *Top* on it
(or just *Top*); with every slot busy it starts when one of them finishes, or *Pause* one that is running.

## Run it

On a machine with an NVIDIA GPU (Docker + NVIDIA Container Toolkit, ~10 GB of GPU memory):

```sh
cd narrator
cp .env.example .env    # KELLYFIN_URL, and KELLYFIN_NARRATOR_TOKEN from Kellyfin: Settings -> Narration
docker compose up -d
```

This starts Whisper, Argos, VieNeu (two instances) and EasyOCR (vi, en, zh, ja, ko) behind nginx on port **8080**,
plus this worker.
The worker connects to Kellyfin by itself — Kellyfin needs no address for it — and reports with every poll whether
each service answers: **Settings → Narration → Workers online** shows ✓/✗ per service. Then queue a library there,
or one video with **Narration** on its page.

The worker only makes outgoing requests (to Kellyfin and to the services), like the AI upscale worker. It can also
run on its own (`docker build -t kellyfin-narrator .`) against the services elsewhere: set `KELLYFIN_URL`,
`KELLYFIN_NARRATOR_TOKEN` and `FILMSUB_URL` (default `http://nginx`).

Other settings (environment): `WORKER_NAME` (unique per worker: a worker asking for work gets back the jobs
still marked as its own, i.e. cut off by a restart), `POLL_SECONDS` (15), `WORK_DIR`, `TRANSLATE_WORKERS` (6),
`EDGE_TTS_PARALLEL` (12, edge-tts lines in flight over every video and voice together), `EDGE_TTS_RETRIES` (3, rounds over the lines edge-tts failed on, after 10, 30, 90 s), `WORKER_JOBS` (3, hard cap; Kellyfin's **Videos at once** setting under Settings → Narration decides, default one at a time).

## CPU only (no NVIDIA GPU)

```sh
docker compose -f docker-compose.cpu.yml up -d     # instead of docker-compose.yml, not next to it
```

Same services and worker, no GPU and no VieNeu: Whisper (int8) and EasyOCR run on the CPU, Vietnamese is read by
edge-tts like the other languages. Queue the **"Whisper · edge-tts only"** or **"OCR · edge-tts only"** pipelines (the
default ones read Vietnamese with VieNeu). Kellyfin only asks for the services a pipeline uses, so a missing VieNeu does
not stop you from queueing. About 6 GB of free memory.
Whisper's model is `WHISPER_MODEL` in `.env`: `medium` (default) or `small` (about 2.5 times faster, a little less exact).

What it costs, measured on a 12-thread desktop CPU with short samples (real films will differ): Whisper `medium`
0.25 × the length of the speech (`small` 0.09 ×), so about 11 min for a 45-minute episode; EasyOCR about 1 s for each
frame it reads, and it reads only the frames where the subtitle band changed (the 40 s test video took 32 s).
Translation (Argos) and edge-tts are the same as with a GPU. VieNeu also runs on the CPU (8 lines, 41 s of voice in
18 s, 2.2 GiB) if you want it: add the `vieneu1` service of `docker-compose.yml` with `VIENEU_DEVICE=CPU`, no GPU
reservation and a volume on `/home/app/.cache/huggingface` (it downloads its model on the first start).

## Languages

Whisper hears many languages; translation needs an Argos model for the spoken language. The compose loads
`en,vi,zh,ja,ko,ms,es` — add codes to `LT_LOAD_ONLY` in `docker-compose.yml` (e.g. `de,fr`) for others.
