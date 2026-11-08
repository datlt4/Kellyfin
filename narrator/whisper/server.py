"""The film-sub Whisper service (hienanh/whisper's /app/server.py) with one change: a transcription no longer blocks the
event loop. In the image `transcribe` is `async def` and runs the (minutes long, on a CPU) transcription inside the
loop, so while it runs the service answers nothing, not even /health, and a second request is not even read: the worker
saw "health does not answer" and nginx 504s. Here the endpoint is a plain `def` (FastAPI runs it in a thread), one
transcription at a time under `_lock` (the next request waits, as before), and /health answers at once.
A request may carry an `X-Request-Id`; POST /v1/cancel/<id> then stops it: at once while it waits for its turn, within a
few seconds (the next segment) while it runs. The worker does that when its job is deleted or paused, so a run nobody
wants any more does not hold the CPU (a transcription cannot otherwise be stopped).
Mount it over the image's file: see docker-compose.yml."""
from fastapi import FastAPI, File, Form, Header, HTTPException, UploadFile
from fastapi.responses import PlainTextResponse
import os
import tempfile
import threading

app = FastAPI(title="whisper-remote")
_model = None
_lock = threading.Lock()
_cancel = {}  # X-Request-Id -> Event, set by /v1/cancel


def get_model(name: str):
    global _model
    if _model is None or _model[0] != name:
        from faster_whisper import WhisperModel
        import shutil
        device = "cuda" if shutil.which("nvidia-smi") else "cpu"
        try:
            import subprocess
            has_gpu = subprocess.run(["nvidia-smi"], capture_output=True).returncode == 0
            device = "cuda" if has_gpu else "cpu"
        except FileNotFoundError:
            device = "cpu"
        compute = "float16" if device == "cuda" else "int8"
        print(f"[+] load model '{name}' on {device} ({compute})...", flush=True)
        m = WhisperModel(name, device=device, compute_type=compute)
        _model = (name, m, device)
        print(f"[+] ready ({device})", flush=True)
    return _model


def fmt(s: float) -> str:
    h = int(s // 3600)
    m = int((s % 3600) // 60)
    sec = int(s % 60)
    ms = int(round((s % 1) * 1000))
    return f"{h:02d}:{m:02d}:{sec:02d},{ms:03d}"


@app.get("/health")
def health():
    name = os.environ.get("WHISPER_MODEL", "medium")
    if _model is not None:
        device = _model[2]
    else:  # not loaded yet: the first transcription loads it; say which device it will use without waiting for that
        import shutil
        import subprocess
        try:
            device = "cuda" if shutil.which("nvidia-smi") and subprocess.run(["nvidia-smi"], capture_output=True).returncode == 0 else "cpu"
        except OSError:
            device = "cpu"
    return {"ok": True, "model": name, "device": device}


@app.post("/v1/cancel/{rid}")
def cancel(rid: str):
    ev = _cancel.get(rid)
    if ev:
        ev.set()
    return {"cancelled": ev is not None}


@app.post("/v1/transcribe", response_class=PlainTextResponse)
def transcribe(file: UploadFile = File(...), model: str = Form("medium"), language: str = Form("en"),
               x_request_id: str = Header(None)):
    suffix = os.path.splitext(file.filename or "audio")[1] or ".wav"
    with tempfile.NamedTemporaryFile(delete=False, suffix=suffix) as tmp:
        tmp.write(file.file.read())
        path = tmp.name
    ev = threading.Event()
    if x_request_id:
        _cancel[x_request_id] = ev
    try:
        # One at a time; a second request waits here, in its own thread, and can be cancelled while it does.
        while not _lock.acquire(timeout=1):
            if ev.is_set():
                raise HTTPException(409, "cancelled")
        try:
            return _run(path, model, language, ev)
        finally:
            _lock.release()
    finally:
        _cancel.pop(x_request_id, None)
        try:
            os.unlink(path)
        except OSError:
            pass


def _run(path, model, language, ev):
    _, wm, _ = get_model(model)
    lang = (language or "en").strip().lower()
    if lang in ("auto", ""):
        lang = None  # de whisper tu detect
    segments, info = wm.transcribe(
        path, language=lang or "en", beam_size=5, vad_filter=True,
        vad_parameters=dict(min_silence_duration_ms=500),
        condition_on_previous_text=False,
    )
    out = []
    idx = 0
    for seg in segments:
        if ev.is_set():
            raise HTTPException(409, "cancelled")  # the segments are made as we go: stopping here stops the work
        text = seg.text.strip()
        if not text.strip().replace(".", "").replace(",", "").replace(" ", ""):
            continue
        idx += 1
        out.append(f"{idx}\n{fmt(seg.start)} --> {fmt(seg.end)}\n{text}\n")
    return "\n".join(out) + ("\n" if out else "")
