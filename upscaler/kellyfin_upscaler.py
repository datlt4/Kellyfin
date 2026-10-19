#!/usr/bin/env python3
"""Kellyfin AI upscale worker (experimental).

Runs on a machine with a GPU, polls the Kellyfin server for jobs, upscales the video and uploads the result,
which then appears as an extra quality in the player. Only the standard library and ffmpeg are needed.

Models it offers:
  lanczos           stub: ffmpeg Lanczos resize + light sharpening (always available, tests the pipeline)
  realesrgan-anime  Real-ESRGAN (realesr-animevideov3) via realesrgan-ncnn-vulkan, if that binary is on PATH
  <name>            any executable models/<name>.sh: called as `<script> INPUT OUTPUT TARGET_HEIGHT`, must write
                    an MP4 to OUTPUT (put your trained model here)

Environment: KELLYFIN_URL, KELLYFIN_UPSCALER_TOKEN (Settings -> AI upscale), WORKER_NAME, TARGET_HEIGHT (1080),
POLL_SECONDS (15), WORK_DIR (temp dir), MODELS_DIR (./models).
"""
import http.client
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

SERVER = os.environ["KELLYFIN_URL"].rstrip("/")
TOKEN = os.environ["KELLYFIN_UPSCALER_TOKEN"]
NAME = os.environ.get("WORKER_NAME") or socket.gethostname()
TARGET = int(os.environ.get("TARGET_HEIGHT", "1080"))
POLL = int(os.environ.get("POLL_SECONDS", "15"))
WORK = os.environ.get("WORK_DIR") or tempfile.gettempdir()
MODELS_DIR = os.environ.get("MODELS_DIR") or os.path.join(os.path.dirname(os.path.abspath(__file__)), "models")


def log(*a):
    print(time.strftime("%F %T"), *a, flush=True)


def api(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(f"{SERVER}/api{path}", data=data, method=method,
                                 headers={"X-Upscaler-Token": TOKEN, "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        raw = r.read()
        return r.status, (json.loads(raw) if raw else None)


# ---------- encoders and models ----------

def _encoder():
    """NVENC when this ffmpeg can actually open it, else x264."""
    try:
        subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "color=s=256x144:d=0.2", "-c:v", "h264_nvenc",
                        "-f", "null", "-"], check=True, capture_output=True, timeout=30)
        return ["-c:v", "h264_nvenc", "-preset", "p5", "-cq", "21"]
    except Exception:
        return ["-c:v", "libx264", "-preset", "medium", "-crf", "20"]


ENCODER = _encoder()


def target_size(w, h):
    """Short side = TARGET (never down), even dimensions, aspect kept."""
    if not w or not h:
        return 0, 0  # unknown size: keep the source's
    short = min(w, h)
    scale = max(1.0, TARGET / short)
    even = lambda x: max(2, int(round(x * scale / 2)) * 2)
    return even(w), even(h)


def run_ffmpeg(args, duration, on_progress):
    """ffmpeg with -progress on stdout, reporting 0..1 against the source duration."""
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-nostdin", "-y", "-progress", "pipe:1", "-nostats", *args],
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    for line in p.stdout:
        if line.startswith("out_time_us=") and duration:
            try:
                on_progress(min(1.0, int(line.split("=")[1]) / 1e6 / duration))
            except ValueError:
                pass
    err = p.stderr.read()
    if p.wait() != 0:
        raise RuntimeError(f"ffmpeg failed: {err.strip()[-800:]}")


def model_lanczos(src, out, job, on_progress):
    w, h = target_size(job.get("width") or 0, job.get("height") or 0)
    vf = f"scale={w}:{h}:flags=lanczos,unsharp=5:5:0.6:5:5:0" if w and h else "unsharp=5:5:0.6:5:5:0"
    run_ffmpeg(["-i", src, "-map", "0:v:0", "-map", "0:a?", "-vf", vf, *ENCODER, "-pix_fmt", "yuv420p",
                "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart", out], job.get("duration"), on_progress)


def model_realesrgan(src, out, job, on_progress):
    """Frames -> realesrgan-ncnn-vulkan (x4) -> video at the source frame rate, then resized to TARGET."""
    tmp = tempfile.mkdtemp(dir=WORK)
    try:
        frames, up = os.path.join(tmp, "in"), os.path.join(tmp, "out")
        os.makedirs(frames)
        os.makedirs(up)
        fps = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=r_frame_rate",
                              "-of", "csv=p=0", src], capture_output=True, text=True).stdout.strip() or "25"
        run_ffmpeg(["-i", src, "-map", "0:v:0", os.path.join(frames, "%08d.png")], job.get("duration"),
                   lambda p: on_progress(p * 0.1))
        subprocess.run(["realesrgan-ncnn-vulkan", "-i", frames, "-o", up, "-n", "realesr-animevideov3", "-s", "4",
                        "-f", "png"], check=True, capture_output=True)
        on_progress(0.8)
        w, h = target_size(job.get("width") or 0, job.get("height") or 0)
        run_ffmpeg(["-framerate", fps, "-i", os.path.join(up, "%08d.png"), "-i", src, "-map", "0:v", "-map", "1:a?",
                    "-vf", f"scale={w}:{h}:flags=lanczos" if w and h else "null", *ENCODER, "-pix_fmt", "yuv420p",
                    "-c:a", "aac", "-b:a", "192k", "-shortest", "-movflags", "+faststart", out],
                   job.get("duration"), lambda p: on_progress(0.8 + p * 0.2))
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def script_model(script):
    def run(src, out, job, on_progress):
        on_progress(0.05)
        subprocess.run([script, src, out, str(TARGET)], check=True)
    return run


def available_models():
    models = {"lanczos": ("Lanczos resize + sharpen (stub, no AI)", model_lanczos)}
    if shutil.which("realesrgan-ncnn-vulkan"):
        models["realesrgan-anime"] = ("Real-ESRGAN realesr-animevideov3 (x4)", model_realesrgan)
    if os.path.isdir(MODELS_DIR):
        for f in sorted(os.listdir(MODELS_DIR)):
            path = os.path.join(MODELS_DIR, f)
            if f.endswith(".sh") and os.access(path, os.X_OK):
                models[f[:-3]] = (f"Custom model ({f})", script_model(path))
    return models


# ---------- transfer ----------

def download(job_id, dest):
    req = urllib.request.Request(f"{SERVER}/api/upscaler/jobs/{job_id}/source", headers={"X-Upscaler-Token": TOKEN})
    with urllib.request.urlopen(req, timeout=60) as r, open(dest, "wb") as f:
        shutil.copyfileobj(r, f, 1 << 20)


def upload(job_id, path):
    u = urllib.parse.urlsplit(SERVER)
    conn = (http.client.HTTPSConnection if u.scheme == "https" else http.client.HTTPConnection)(u.netloc, timeout=600)
    with open(path, "rb") as f:
        conn.request("PUT", f"{u.path}/api/upscaler/jobs/{job_id}/result", body=f,
                     headers={"X-Upscaler-Token": TOKEN, "Content-Type": "video/mp4",
                              "Content-Length": str(os.path.getsize(path))})
        r = conn.getresponse()
        r.read()
    if r.status >= 300:
        raise RuntimeError(f"upload failed: HTTP {r.status}")


def process(job, models):
    jid = job["job_id"]
    _, fn = models[job["model"]]
    tmp = tempfile.mkdtemp(prefix=f"kellyfin-{jid}-", dir=WORK)
    last = [0.0]

    def on_progress(p):
        if time.time() - last[0] > 5:
            last[0] = time.time()
            try:
                api("POST", f"/upscaler/jobs/{jid}/progress", {"progress": p})
            except Exception:
                pass

    try:
        src, out = os.path.join(tmp, "source"), os.path.join(tmp, "out.mp4")
        log(f"job {jid}: item {job['item_id']} with {job['model']} — downloading")
        download(jid, src)
        log(f"job {jid}: upscaling")
        fn(src, out, job, on_progress)
        log(f"job {jid}: uploading {os.path.getsize(out) >> 20} MB")
        upload(jid, out)
        log(f"job {jid}: done")
    except Exception as e:
        log(f"job {jid}: failed: {e}")
        try:
            api("POST", f"/upscaler/jobs/{jid}/fail", {"error": str(e)})
        except Exception:
            pass
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main():
    log(f"worker {NAME} -> {SERVER}, target {TARGET}p, encoder {ENCODER[1]}")
    while True:
        models = available_models()
        try:
            status, job = api("POST", "/upscaler/claim",
                              {"worker": NAME, "models": [{"name": n, "description": d} for n, (d, _) in models.items()]})
        except urllib.error.HTTPError as e:
            log(f"server refused: HTTP {e.code} (check KELLYFIN_UPSCALER_TOKEN)")
            time.sleep(POLL * 4)
            continue
        except Exception as e:
            log(f"server unreachable: {e}")
            time.sleep(POLL)
            continue
        if status == 200 and job:
            process(job, models)
        else:
            time.sleep(POLL)


if __name__ == "__main__":
    sys.exit(main())
