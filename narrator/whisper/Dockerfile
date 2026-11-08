# The film-sub Whisper service: hienanh/whisper (faster-whisper, models baked in) with its server.py replaced by the one
# next to this file, which keeps /health answering during a transcription and can cancel a run (see server.py).
FROM hienanh/whisper:latest
COPY server.py /app/server.py
