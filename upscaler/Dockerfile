# Kellyfin AI upscale worker. Debian's ffmpeg includes NVENC; the GPU comes from the NVIDIA container toolkit.
FROM python:3.12-slim-bookworm
RUN apt-get update \
 && apt-get install -y --no-install-recommends ffmpeg ca-certificates \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY kellyfin_upscaler.py ./
COPY models ./models
ENV PYTHONUNBUFFERED=1
CMD ["python", "kellyfin_upscaler.py"]
