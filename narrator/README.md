# Narration worker (thuyết minh)

Gives videos a Vietnamese and/or Chinese voice-over, as an extra audio track in Kellyfin, plus subtitles in the
spoken language, English, Vietnamese and Chinese. The video files are never changed.

Per video, in four stages (shown in **Settings → Narration → Queue**):

1. **Subtitles** in the spoken language — the video's own, else [Whisper](https://github.com/SYSTRAN/faster-whisper)
   (pick the language when queueing, or let Argos detect it);
2. **Translation** into English, Vietnamese and Chinese with Argos / LibreTranslate (subtitles the video already has
   are reused);
3. **Voice**: VieNeu-TTS reads Vietnamese, edge-tts reads Chinese (edge-tts is a Microsoft service: needs Internet);
4. **Mux**: the voice over the original sound, which ducks while the voice speaks.

## Run it

On a machine with an NVIDIA GPU (Docker + NVIDIA Container Toolkit, ~10 GB of GPU memory):

```sh
cd narrator/film-sub
cp .env.example .env    # KELLYFIN_URL, and KELLYFIN_NARRATOR_TOKEN from Kellyfin: Settings -> Narration
docker compose up -d
```

`film-sub/` starts Whisper, Argos, VieNeu (two instances) and EasyOCR behind nginx on port **8080**, plus this
worker. In Kellyfin, **Settings → Narration**: enter `http://<this machine>:8080`, **Save**, **Check server** (every
service must show ✓). Then queue a library there, or one video with **Narration** on its page.

The worker only makes outgoing requests (to Kellyfin and to the services), like the AI upscale worker. It can also
run on its own (`docker build -t kellyfin-narrator .`) against film-sub services elsewhere: set `KELLYFIN_URL`,
`KELLYFIN_NARRATOR_TOKEN`, and optionally `FILMSUB_URL` (otherwise the URL from Kellyfin's settings is used).

Other settings (environment): `WORKER_NAME`, `POLL_SECONDS` (15), `WORK_DIR`, `TRANSLATE_WORKERS` (6),
`EDGE_TTS_PARALLEL` (12).

## Languages

Whisper hears many languages; translation needs an Argos model for the spoken language. `film-sub` loads
`en,vi,zh,ja,ko,ms,es` — add codes to `LT_LOAD_ONLY` in `film-sub/docker-compose.yml` (e.g. `de,fr`) for others.
