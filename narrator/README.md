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
2. **Translation** with Argos / LibreTranslate into the languages the graph asks for (subtitles the video already has
   in a language are reused);
3. **Voice**: VieNeu-TTS reads Vietnamese, edge-tts any language (edge-tts is a Microsoft service: needs Internet);
4. **Mux**, on the Kellyfin server: the voice over the original sound, which ducks while the voice speaks. The worker
   hands the voice over and takes the next video meanwhile.

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
`EDGE_TTS_PARALLEL` (12).

## Languages

Whisper hears many languages; translation needs an Argos model for the spoken language. The compose loads
`en,vi,zh,ja,ko,ms,es` — add codes to `LT_LOAD_ONLY` in `docker-compose.yml` (e.g. `de,fr`) for others.
