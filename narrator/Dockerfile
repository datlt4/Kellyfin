# Kellyfin narration worker: ffmpeg for audio work, requests/numpy/edge-tts for the pipeline. No GPU needed here:
# Whisper, Argos and VieNeu run in the film-sub services.
FROM python:3.12-slim-bookworm
RUN apt-get update \
 && apt-get install -y --no-install-recommends ffmpeg ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && pip install --no-cache-dir requests numpy edge-tts pillow
WORKDIR /app
COPY kellyfin_narrator.py ./
ENV PYTHONUNBUFFERED=1
CMD ["python", "kellyfin_narrator.py"]
