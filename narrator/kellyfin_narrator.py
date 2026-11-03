#!/usr/bin/env python3
"""Kellyfin narration worker (thuyết minh).

Runs in the film-sub compose next to Whisper, Argos and VieNeu, and polls Kellyfin for jobs. The original sound
comes from Kellyfin (the video file never leaves the server and is never changed). Per video, four stages:
  1. subtitles in the spoken language: the video's own, else Whisper ("auto": Argos detects the language);
  2. translation into English, Vietnamese and Chinese (Argos), unless the video has them;
  3. a voice reads each narration language on the subtitle timeline: VieNeu (vi), edge-tts (zh, needs Internet);
  4. mux — on the Kellyfin server: the worker uploads each voice alone (FLAC) and takes the next video at once,
     while Kellyfin mixes the voice over the ducked original sound from the video file.
Subtitles and tracks go back to Kellyfin, which offers them as extra subtitle / audio tracks. Every step that needs a
service checks it first and fails the job with a clear message when it does not answer.

It reaches Kellyfin itself (Kellyfin never calls it or the services) and reports, with every poll, whether the
services answer; Settings -> Narration shows that report.

Environment: KELLYFIN_URL, KELLYFIN_NARRATOR_TOKEN (Settings -> Narration), FILMSUB_URL (http://nginx: the film-sub
compose's nginx), WORKER_NAME, POLL_SECONDS (15), WORK_DIR, TRANSLATE_WORKERS (6), EDGE_TTS_PARALLEL (12).
"""
import asyncio
import json
import os
import re
import shutil
import socket
import subprocess
import tempfile
import time
import uuid
from concurrent.futures import ThreadPoolExecutor
from difflib import SequenceMatcher

import numpy as np
import requests

SERVER = os.environ["KELLYFIN_URL"].rstrip("/")
TOKEN = os.environ["KELLYFIN_NARRATOR_TOKEN"]
NAME = os.environ.get("WORKER_NAME") or socket.gethostname()
POLL = int(os.environ.get("POLL_SECONDS", "15"))
FILMSUB = (os.environ.get("FILMSUB_URL") or "http://nginx").rstrip("/")
WORK = os.environ.get("WORK_DIR") or tempfile.gettempdir()
TRANSLATE_WORKERS = int(os.environ.get("TRANSLATE_WORKERS", "6"))
EDGE_PARALLEL = int(os.environ.get("EDGE_TTS_PARALLEL", "12"))
SR = 48000


def log(*a):
    print(time.strftime("%F %T"), *a, flush=True)


class JobError(Exception):
    """A failure worth showing in Settings as is."""


# ---------- Kellyfin ----------

kf = requests.Session()
kf.headers["X-Narrator-Token"] = TOKEN


def kf_url(path):
    return f"{SERVER}/api{path}"


def retried(fn):
    """Kellyfin may restart (an update) while a job runs: try again for about a minute before giving up."""
    def run(*a, **kw):
        for attempt in range(6):
            try:
                return fn(*a, **kw)
            except (requests.ConnectionError, requests.Timeout) as e:
                if attempt == 5:
                    raise JobError(f"Kellyfin does not answer: {e}") from e
                time.sleep(10)
    return run


@retried
def kf_json(method, path, body=None):
    r = kf.request(method, kf_url(path), json=body, timeout=60)
    r.raise_for_status()
    return r.json() if r.content else None


@retried
def kf_download(path, dest):
    with kf.get(kf_url(path), stream=True, timeout=(30, 600)) as r:
        r.raise_for_status()
        with open(dest, "wb") as f:
            for chunk in r.iter_content(1 << 20):
                f.write(chunk)


@retried
def kf_upload(jid, name, src):
    with open(src, "rb") as f:
        kf.put(kf_url(f"/narrator/jobs/{jid}/files/{name}"), data=f, timeout=(30, 1800)).raise_for_status()


# ---------- subtitles ----------

TIME = re.compile(r"(\d+):(\d+):(\d+)[,.](\d+)\s*-->\s*(\d+):(\d+):(\d+)[,.](\d+)")


def parse_srt(text):
    """[(start, end, text)] in seconds."""
    cues = []
    for block in re.split(r"\n\s*\n", text.replace("\r", "").strip()):
        lines = block.splitlines()
        i = next((k for k, l in enumerate(lines) if "-->" in l), None)
        m = TIME.search(lines[i]) if i is not None else None
        body = " ".join(l.strip() for l in lines[i + 1:] if l.strip()) if m else ""
        if not body:
            continue
        g = [int(x) for x in m.groups()]
        cues.append((g[0] * 3600 + g[1] * 60 + g[2] + g[3] / 1000, g[4] * 3600 + g[5] * 60 + g[6] + g[7] / 1000, body))
    return cues


def stamp(t):
    ms = int(round(t * 1000))
    return f"{ms // 3600000:02}:{ms // 60000 % 60:02}:{ms // 1000 % 60:02},{ms % 1000:03}"


def write_srt(cues, path):
    with open(path, "w", encoding="utf-8") as f:
        for n, (s, e, t) in enumerate(cues, 1):
            f.write(f"{n}\n{stamp(s)} --> {stamp(e)}\n{t}\n\n")


# ---------- film-sub services (each checked before use) ----------

def need(base, path, what):
    try:
        r = requests.get(f"{base}{path}", timeout=15)
        r.raise_for_status()
        return r
    except Exception as e:
        raise JobError(f"{what} at {base}{path} does not answer: {e}") from e


def whisper(base, audio, lang, tmp):
    need(base, "/whisper/health", "Whisper")
    wav = os.path.join(tmp, "speech.wav")
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", audio, "-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", wav],
                   check=True)
    with open(wav, "rb") as f:
        r = requests.post(f"{base}/whisper/v1/transcribe", files={"file": ("speech.wav", f, "audio/wav")},
                          data={"model": "medium", "language": lang}, timeout=3600)
    r.raise_for_status()
    cues = dedupe(parse_srt(r.text))
    if not cues:
        raise JobError("Whisper found no speech in this video")
    return cues


def dedupe(cues, similar=0.6, window=3, gap=10.0):
    """Whisper often hears one line two or three times in a row with a character or two changed. A line more than
    60 % like one of the last few kept (and close in time) is dropped; the kept one stays on screen until it ends."""
    out = []
    for s, e, t in cues:
        key = re.sub(r"\W+", " ", t.lower()).strip()
        for k in range(len(out) - 1, max(-1, len(out) - 1 - window), -1):
            ps, pe, pt = out[k]
            if s - pe <= gap and SequenceMatcher(None, re.sub(r"\W+", " ", pt.lower()).strip(), key).ratio() > similar:
                out[k] = (ps, max(pe, e), pt)
                break
        else:
            out.append((s, e, t))
    return out


def argos_code(base, lang):
    """Argos' code for a language ("zh" is "zh-Hans" there); a clear error when no model is loaded for it."""
    codes = [l["code"] for l in need(base, "/translate/languages", "Argos").json()]
    code = lang if lang in codes else next((c for c in codes if c.split("-")[0] == lang), None)
    if not code:
        raise JobError(f"Argos has no {lang} model (it offers {', '.join(codes)}; add it to LT_LOAD_ONLY in film-sub)")
    return code


def detect(base, cues):
    """Spoken language of Whisper's text (it does not say which one it heard)."""
    need(base, "/translate/languages", "Argos")
    sample = " ".join(c[2] for c in cues[: min(len(cues), 40)])[:2000]
    r = requests.post(f"{base}/translate/detect", json={"q": sample}, timeout=60)
    r.raise_for_status()
    found = r.json()
    if not found:
        raise JobError("Argos could not tell the spoken language; set it when queueing")
    return found[0]["language"].split("-")[0]


def translate(base, cues, source, lang, on_progress):
    src, target = argos_code(base, source), argos_code(base, lang)
    s = requests.Session()
    done = [0]

    def one(cue):
        r = s.post(f"{base}/translate/translate", json={"q": cue[2], "source": src, "target": target, "format": "text"},
                   timeout=120)
        r.raise_for_status()
        done[0] += 1
        if done[0] % 25 == 0:
            on_progress(done[0] / len(cues))
        return (cue[0], cue[1], (r.json().get("translatedText") or "").strip() or cue[2])

    with ThreadPoolExecutor(TRANSLATE_WORKERS) as ex:
        out = list(ex.map(one, cues))
    if all(a[2] == b[2] for a, b in zip(out, cues)):
        raise JobError(f"Argos returned the {source} text unchanged for {lang}")
    return out


class VieNeu:
    """Gradio HTTP API of VieNeu-TTS; nginx balances two instances, so one session keeps the sticky cookie."""

    def __init__(self, base):
        self.base = f"{base}/gradio_api"
        self.s = requests.Session()

    def run(self, endpoint, data, on_progress=None):
        r = self.s.post(f"{self.base}/call{endpoint}", json={"data": data}, timeout=60)
        r.raise_for_status()
        eid = r.json()["event_id"]
        event = None
        with self.s.get(f"{self.base}/call{endpoint}/{eid}", stream=True, timeout=(60, 900)) as st:
            st.raise_for_status()
            for raw in st.iter_lines(decode_unicode=True):
                line = (raw or "").strip()
                if line.startswith("event:"):
                    event = line[6:].strip()
                elif line.startswith("data:"):
                    try:
                        payload = json.loads(line[5:].strip())
                    except json.JSONDecodeError:
                        payload = line[5:].strip()
                    if event == "complete":
                        return payload if isinstance(payload, list) else [payload]
                    if event == "error":
                        raise JobError(f"VieNeu: {payload}")
                    if event == "generating" and on_progress and isinstance(payload, list) and len(payload) > 1:
                        m = re.search(r"(\d+)\s*/\s*(\d+)", str(payload[1]))
                        if m and int(m.group(2)):
                            on_progress(int(m.group(1)) / int(m.group(2)))
        raise JobError("VieNeu ended without a result")

    def voices(self):
        out = self.run("/_srt_voices", [])
        return [c[1] if isinstance(c, list) else c for c in (out[0] or {}).get("choices") or []]

    def speak(self, srt, voice, dest, on_progress):
        with open(srt, "rb") as f:
            r = self.s.post(f"{self.base}/upload", params={"upload_id": uuid.uuid4().hex},
                            files={"files": ("narration.srt", f, "application/x-subrip")}, timeout=60)
        r.raise_for_status()
        file_arg = {"path": r.json()[0], "orig_name": "narration.srt", "meta": {"_type": "gradio.FileData"}}
        out = self.run("/wrapper_2", [file_arg, voice, True, "wav", True, 32], on_progress)
        audio = out[0] if out else None
        if not isinstance(audio, dict):
            raise JobError(f"VieNeu returned no audio: {out[1] if len(out) > 1 else out}")
        url = audio.get("url") or f"{self.base}/file={audio['path']}"
        with self.s.get(url, stream=True, timeout=(60, 600)) as d:
            d.raise_for_status()
            with open(dest, "wb") as f:
                for chunk in d.iter_content(1 << 20):
                    f.write(chunk)


def tts_vi(base, srt, voice, dest, on_progress):
    try:
        vn = VieNeu(base)
        voices = vn.voices()
    except JobError:
        raise
    except Exception as e:
        raise JobError(f"VieNeu at {base} does not answer: {e}") from e
    if not voices:
        raise JobError("VieNeu has no model loaded (load it in its web UI)")
    if voice not in voices:
        raise JobError(f"VieNeu has no voice {voice!r}; it offers: {', '.join(voices)}")
    vn.speak(srt, voice, dest, on_progress)


def speakable(text):
    """Whether a line has anything to read: translation can leave lines of punctuation only (".", "…")."""
    return any(ch.isalnum() for ch in text)


def tts_zh(cues, voice, duration, dest, tmp, on_progress):
    import edge_tts

    async def synth(text, out):
        for attempt in (1, 2, 3):
            try:
                await edge_tts.Communicate(text, voice).save(out)
                return
            except Exception:
                await asyncio.sleep(3 * attempt)
        raise JobError(f"edge-tts failed on: {text[:40]}")

    probe = os.path.join(tmp, "probe.mp3")
    try:
        asyncio.run(synth("测试", probe))
    except Exception as e:
        raise JobError(f"edge-tts does not answer (it needs Internet access): {e}") from e
    voice_dir = os.path.join(tmp, "zh_voice")
    os.makedirs(voice_dir, exist_ok=True)
    done, failed = [0], []
    todo = [(i, c[2]) for i, c in enumerate(cues) if speakable(c[2])]

    async def all_cues():
        sem = asyncio.Semaphore(EDGE_PARALLEL)

        async def one(i, text):
            async with sem:
                try:
                    await synth(text, os.path.join(voice_dir, f"{i:05}.mp3"))
                except JobError:
                    failed.append(text)  # one odd line stays silent rather than failing the video
                done[0] += 1
                if done[0] % 20 == 0:
                    on_progress(done[0] / len(todo))

        await asyncio.gather(*(one(i, t) for i, t in todo))

    asyncio.run(all_cues())
    if len(failed) > max(3, len(todo) // 10):
        raise JobError(f"edge-tts failed on {len(failed)} of {len(todo)} lines, e.g. {failed[0][:40]!r}")
    if failed:
        log(f"edge-tts skipped {len(failed)} lines: {[t[:30] for t in failed]}")
    # Every cue on one 48 kHz stereo timeline as long as the video.
    buf = np.zeros(int((duration + 1) * SR) * 2, dtype=np.int32)
    for i, (start, _, _) in enumerate(cues):
        if not os.path.isfile(os.path.join(voice_dir, f"{i:05}.mp3")):
            continue
        pcm = subprocess.run(["ffmpeg", "-v", "error", "-i", os.path.join(voice_dir, f"{i:05}.mp3"), "-f", "s16le",
                              "-ac", "2", "-ar", str(SR), "-"], capture_output=True, check=True).stdout
        a = np.frombuffer(pcm[: len(pcm) // 4 * 4], dtype=np.int16).astype(np.int32)
        off = int(start * SR) * 2
        n = max(0, min(len(a), len(buf) - off))
        buf[off: off + n] += a[:n]
    pcm = np.clip(buf, -32767, 32767).astype(np.int16).tobytes()
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "s16le", "-ac", "2", "-ar", str(SR), "-i", "-", dest],
                   input=pcm, check=True)


def to_flac(voice, dest):
    """The voice alone, lossless (mostly silence, so small); Kellyfin mixes it in."""
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", voice, "-ac", "2", "-ar", str(SR), "-c:a", "flac", dest], check=True)


# ---------- one job ----------

def process(job):
    jid = job["job_id"]
    base = FILMSUB
    langs, subs, duration = job["langs"], job.get("subs") or {}, job.get("duration") or 0
    source = job.get("source") or "auto"
    tmp = tempfile.mkdtemp(prefix=f"narration-{jid}-", dir=WORK)
    # Share of the progress bar per stage: 1 subtitles, 2 translation, 3 voice (4, the mux, is Kellyfin's).
    span = {1: (0.0, 0.3), 2: (0.3, 0.2), 3: (0.5, 0.4)}

    def step(stage, label, frac=0.0):
        a, w = span[stage]
        try:
            kf_json("POST", f"/narrator/jobs/{jid}/progress", {"stage": stage, "step": label, "progress": a + w * frac})
        except Exception:
            pass

    @retried
    def existing(lang):
        r = kf.get(kf_url(f"/narrator/jobs/{jid}/subtitle/{subs[lang]}"), timeout=300)
        r.raise_for_status()
        return parse_srt(r.text)

    def save(lang, cues, upload):
        path = os.path.join(tmp, f"{lang}.srt")
        write_srt(cues, path)
        if upload:
            kf_upload(jid, f"{lang}.srt", path)
        return path

    try:
        log(f"job {jid}: {job['title']} ({source}) -> narration {','.join(langs)} via {base}")

        # 1. Subtitles in the spoken language.
        texts = {}
        targets = [t for t in job.get("targets") or ["en", "vi", "zh"] if t != source]
        targets += [l for l in langs if l not in targets and l != source]
        if source != "auto" and source in subs:
            step(1, f"Using the video's {source} subtitles")
            texts[source] = existing(source)
        elif source == "auto" and subs and all(t in subs for t in targets):
            source = next(iter(subs))  # nothing to make: every target already exists
        else:
            # Only Whisper needs the sound itself.
            step(1, "Fetching the original sound")
            original = os.path.join(tmp, "original.mka")
            kf_download(f"/narrator/jobs/{jid}/audio", original)
            step(1, f"Transcribing {'the spoken language' if source == 'auto' else source} (Whisper)")
            cues = whisper(base, original, source, tmp)
            if source == "auto":
                source = detect(base, cues)
                log(f"job {jid}: spoken language {source}")
            texts[source] = cues
            if source not in subs:
                save(source, cues, upload=True)
        targets = [t for t in targets if t != source]

        # 2. Translation (from the spoken language) into every target the video lacks.
        for n, t in enumerate(targets):
            if t in subs:
                step(2, f"Using the video's {t} subtitles", n / len(targets))
                texts[t] = existing(t)
                continue
            if source not in texts:
                texts[source] = existing(source)
            label = f"Translating {source} → {t} (Argos)"
            step(2, label, n / len(targets))
            texts[t] = translate(base, texts[source], source, t,
                                 lambda f, n=n, label=label: step(2, label, (n + f) / len(targets)))
            save(t, texts[t], upload=True)
        for l in langs:
            if not texts.get(l):
                raise JobError(f"no {l} subtitles to read out")
        if not duration:  # unprobed video: the timeline ends with the last line
            duration = max(c[1] for l in langs for c in texts[l]) + 1

        # 3. Voice per narration language; 4. (mux) happens on the Kellyfin server.
        for n, l in enumerate(langs):
            srt = save(l, [c for c in texts[l] if speakable(c[2])], upload=False)
            voice_wav = os.path.join(tmp, f"{l}.voice.wav")
            label = f"Reading {l} aloud ({'VieNeu' if l == 'vi' else 'edge-tts'})"
            step(3, label, n / len(langs))
            prog = lambda f, n=n, label=label: step(3, label, (n + f) / len(langs))
            if l == "vi":
                tts_vi(base, srt, job["voice_vi"], voice_wav, prog)
            else:
                tts_zh(texts[l], job["voice_zh"], duration, voice_wav, tmp, prog)
            flac = os.path.join(tmp, f"{l}.voice.flac")
            to_flac(voice_wav, flac)
            kf_upload(jid, f"{l}.voice.flac", flac)
            log(f"job {jid}: {l} voice uploaded ({os.path.getsize(flac) >> 20} MB)")
        kf_json("POST", f"/narrator/jobs/{jid}/done")
        log(f"job {jid}: handed to Kellyfin for the mux")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


# ---------- service report sent with every poll ----------

_report = {"at": 0.0, "edge_at": 0.0, "checks": [], "voices": [], "edge": None}


def health():
    """Whisper, Argos, VieNeu about once a minute and edge-tts every five, for Settings -> Narration."""
    now = time.time()
    if now - _report["at"] < 60:
        return _report["checks"], _report["voices"]

    def probe(name, fn):
        try:
            ok, detail = fn()
        except Exception as e:
            ok, detail = False, f"{type(e).__name__}: {e}"
        return {"name": name, "ok": ok, "detail": str(detail)[:300]}

    def whisper_ok():
        # Whisper answers nothing while it transcribes (e.g. another job's request): busy, not down.
        try:
            r = requests.get(f"{FILMSUB}/whisper/health", timeout=20)
        except requests.ReadTimeout:
            return True, "busy (transcribing), reachable"
        r.raise_for_status()
        return True, r.text.strip()

    def argos_ok():
        r = requests.get(f"{FILMSUB}/translate/languages", timeout=10)
        r.raise_for_status()
        codes = [l["code"] for l in r.json()]
        need = all(any(c.split("-")[0] == x for c in codes) for x in ("en", "vi", "zh"))
        return need, ", ".join(codes)

    voices = []

    def vieneu_ok():
        voices.extend(VieNeu(FILMSUB).voices())
        return bool(voices), f"{len(voices)} voices" if voices else "no model loaded"

    def edge_ok():
        if now - _report["edge_at"] >= 300 or _report["edge"] is None:
            import edge_tts

            async def go():
                # Read the whole (short) stream: leaving it early leaks the connection.
                got = False
                async for chunk in edge_tts.Communicate("测试", "zh-CN-XiaoxiaoNeural").stream():
                    got = got or chunk["type"] == "audio"
                return got

            _report["edge"] = asyncio.run(asyncio.wait_for(go(), 20))
            _report["edge_at"] = now
        return _report["edge"], "Microsoft speech service reachable" if _report["edge"] else "no audio"

    checks = [probe("Whisper (subtitles)", whisper_ok), probe("Argos (translation)", argos_ok),
              probe("VieNeu (Vietnamese voice)", vieneu_ok), probe("edge-tts (Chinese voice, Internet)", edge_ok)]
    _report.update(at=now, checks=checks, voices=voices)
    return checks, voices


def main():
    log(f"worker {NAME} -> {SERVER}, film-sub {FILMSUB}")
    while True:
        checks, voices = health()
        try:
            r = kf.post(kf_url("/narrator/claim"), json={"worker": NAME, "checks": checks, "voices": voices}, timeout=60)
            if r.status_code in (401, 403):
                log("server refused the token (KELLYFIN_NARRATOR_TOKEN, Settings -> Narration)")
                time.sleep(POLL * 4)
                continue
            r.raise_for_status()
        except Exception as e:
            log(f"server unreachable: {e}")
            time.sleep(POLL)
            continue
        if r.status_code != 200:
            time.sleep(POLL)
            continue
        job = r.json()
        try:
            process(job)
        except Exception as e:
            msg = str(e) if isinstance(e, JobError) else f"{type(e).__name__}: {e}"
            log(f"job {job['job_id']}: failed: {msg}")
            try:
                kf_json("POST", f"/narrator/jobs/{job['job_id']}/fail", {"error": msg})
            except Exception:
                pass


if __name__ == "__main__":
    main()
