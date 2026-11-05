#!/usr/bin/env python3
"""Kellyfin narration worker (thuyết minh).

Runs in the film-sub compose next to Whisper, Argos and VieNeu, and polls Kellyfin for jobs. The original sound
comes from Kellyfin. Each job carries a pipeline graph (Kellyfin's pipeline.rs) run in four stages:
  1. subtitles: the video's own, Whisper ("auto": Argos detects the language), or OCR of burned-in subtitles;
  2. translation (Argos) into the languages the graph asks for, unless the video has them;
  3. voices on the subtitle timeline: VieNeu (Vietnamese) or edge-tts (any language, needs Internet);
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
import contextlib
import json
import os
import re
import shutil
import socket
import subprocess
import tempfile
import threading
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
WHISPER_MODEL = os.environ.get("WHISPER_MODEL") or "medium"  # baked into the Whisper image: tiny, small, medium
WHISPER_TIMEOUT = int(os.environ.get("WHISPER_TIMEOUT") or 4 * 3600)  # seconds per video (a CPU takes long)
WORKER_JOBS = int(os.environ.get("WORKER_JOBS") or 3)  # videos worked on at once (see "RES")
SR = 48000

# One video at a time in each of these (they are heavy), so while one video is read aloud (edge-tts, mostly waiting
# for the Internet) the next can already be in OCR, Whisper or translation.
RES = {name: threading.Lock() for name in ("whisper", "ocr", "translate", "vieneu")}


def log(*a):
    print(time.strftime("%F %T"), *a, flush=True)


class JobError(Exception):
    """A failure worth showing in Settings as is."""


class JobCancelled(Exception):
    """The job was deleted or paused in Settings: stop, leave nothing behind, report nothing."""


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
                          data={"model": WHISPER_MODEL, "language": lang}, timeout=WHISPER_TIMEOUT)
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


# edge-tts voice per language when a voice node names none (Chinese: the one set in Kellyfin).
EDGE_VOICES = {
    "vi": "vi-VN-HoaiMyNeural", "en": "en-US-AriaNeural", "ja": "ja-JP-NanamiNeural", "ko": "ko-KR-SunHiNeural",
    "de": "de-DE-KatjaNeural", "fr": "fr-FR-DeniseNeural", "es": "es-ES-ElviraNeural", "it": "it-IT-ElsaNeural",
    "pt": "pt-BR-FranciscaNeural", "ru": "ru-RU-SvetlanaNeural", "th": "th-TH-PremwadeeNeural",
    "id": "id-ID-GadisNeural", "ms": "ms-MY-YasminNeural", "zh": "zh-CN-XiaoxiaoNeural",
}


def tts_edge(cues, voice, duration, dest, tmp, on_progress):
    """edge-tts per line, each placed at its start on one timeline."""
    import edge_tts

    async def synth(text, out):
        for attempt in (1, 2, 3):
            try:
                await edge_tts.Communicate(text, voice).save(out)
                return
            except Exception:
                await asyncio.sleep(3 * attempt)
        # A failed save can leave a partial file behind: it must not reach the timeline.
        try:
            os.unlink(out)
        except OSError:
            pass
        raise JobError(f"edge-tts failed on: {text[:40]}")

    probe = os.path.join(tmp, "probe.mp3")
    try:
        asyncio.run(synth("1", probe))
    except Exception as e:
        raise JobError(f"edge-tts does not answer (it needs Internet access): {e}") from e
    voice_dir = os.path.join(tmp, f"voice-{uuid.uuid4().hex[:8]}")
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
    def decode(mp3):
        r = subprocess.run(["ffmpeg", "-v", "error", "-i", mp3, "-f", "s16le", "-ac", "2", "-ar", str(SR), "-"],
                           capture_output=True)
        return r.stdout if r.returncode == 0 else b""

    # A truncated mp3 (the stream broke off) is made again once; still bad, the line stays silent.
    redo = [(i, c[2]) for i, c in enumerate(cues) if os.path.isfile(os.path.join(voice_dir, f"{i:05}.mp3"))
            and not decode(os.path.join(voice_dir, f"{i:05}.mp3"))]
    if redo:
        log(f"edge-tts: {len(redo)} truncated lines, making them again")

        async def again():
            await asyncio.gather(*(synth(t, os.path.join(voice_dir, f"{i:05}.mp3")) for i, t in redo), return_exceptions=True)

        asyncio.run(again())
    unreadable = []
    for i, (start, _, text) in enumerate(cues):
        mp3 = os.path.join(voice_dir, f"{i:05}.mp3")
        if not os.path.isfile(mp3):
            continue
        pcm = decode(mp3)
        if not pcm:
            unreadable.append(text)
            continue
        a = np.frombuffer(pcm[: len(pcm) // 4 * 4], dtype=np.int16).astype(np.int32)
        off = int(start * SR) * 2
        n = max(0, min(len(a), len(buf) - off))
        buf[off: off + n] += a[:n]
    if unreadable:
        log(f"edge-tts audio unreadable for {len(unreadable)} lines (left silent): {[t[:30] for t in unreadable]}")
        if len(failed) + len(unreadable) > max(3, len(todo) // 10):
            raise JobError(f"edge-tts gave no usable audio for {len(failed) + len(unreadable)} of {len(todo)} lines")
    pcm = np.clip(buf, -32767, 32767).astype(np.int16).tobytes()
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "s16le", "-ac", "2", "-ar", str(SR), "-i", "-", dest],
                   input=pcm, check=True)


def to_flac(voice, dest):
    """The voice alone, lossless (mostly silence, so small); Kellyfin mixes it in."""
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", voice, "-ac", "2", "-ar", str(SR), "-c:a", "flac", dest], check=True)


# ---------- one job ----------

# ---------- OCR of burned-in subtitles ----------

# Watermarks and site captions that OCR picks up and a voice must not read.
OCR_JUNK = re.compile(r"sohu|iqiyi|youku|\bqq\b|bilibili|tencent|mgtv|https?:|www\.|\.com|1905", re.I)
DIFF_HI = 4.0      # mean change (0-255) of the subtitle band, at 320 px wide, that means "new text"
DIFF_CELL = 12.0   # ...or this mean change in any 48 x 8 px cell (a short caption over a still picture)
RECHECK = 10       # frames (5 s at 2 fps) after which a shown line is looked at again anyway
OCR_WIDTH = 1920   # px: the band is read at the video's own size up to this (captions are small in a wide frame)
CENTER = 0.15      # burned-in subtitles are centred: a line's middle may stray this share of the width
STILL_SPAN = 10.0  # seconds: a line shown this long, in one place, is page text, not a subtitle
STILL_GAP = 20.0   # a caption box may hide the line this long without it counting as a new line
STILL_SHARE = 0.35 # ...but it must be visible at least this share of the span


def ocr(base, video, langs, crop, duration, on_progress):
    """Lines from the bottom `crop` of the frame, sampled twice a second; a frame is read only when the band
    changed, once per language in `langs` (overlapping boxes keep the surer reading). Lines over 60 % alike are one
    line; shorter than 0.7 s dropped; the same text within 2 s merged. Text behind the picture (a lecture's slides,
    a status bar, a list) is dropped: lines that stay in one place for 10 s+, and captions
    that are not centred (skipped when that leaves nothing)."""
    lang = langs[0]
    need(base, "/ocr/health", "EasyOCR")
    import io
    from PIL import Image

    w, h = (int(x) for x in subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0", video],
        capture_output=True, text=True, check=True).stdout.strip().split(",")[:2])
    band = max(16, int(h * (crop or 0.35)) // 2 * 2)
    ow = min(OCR_WIDTH, w) // 2 * 2
    oh = max(16, int(band * ow / w) // 2 * 2)
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-nostdin", "-i", video, "-vf",
                          f"fps=2,crop=iw:{band}:0:ih-{band},scale={ow}:{oh}", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
                         stdout=subprocess.PIPE)
    size, step = ow * oh * 3, max(1, ow // 320)
    cols = ow // step
    s = requests.Session()

    def overlap(a, b):
        ix = max(0, min(a[2], b[2]) - max(a[0], b[0]))
        iy = max(0, min(a[3], b[3]) - max(a[1], b[1]))
        small = min((a[2] - a[0]) * (a[3] - a[1]), (b[2] - b[0]) * (b[3] - b[1])) or 1
        return ix * iy / small

    def read(frame):
        buf = io.BytesIO()
        Image.frombytes("RGB", (ow, oh), frame).save(buf, "JPEG", quality=92)
        found = []
        for l in langs:
            r = s.post(f"{base}/ocr/ocr", files={"file": ("f.jpg", buf.getvalue(), "image/jpeg")},
                       data={"lang": l, "min_conf": "0.4"}, timeout=120)
            r.raise_for_status()
            for x in r.json().get("results") or []:
                same = next((k for k, y in enumerate(found) if overlap(x["bbox"], y["bbox"]) > 0.5), None)
                if same is None:
                    found.append(x)
                elif x.get("conf", 0) > found[same].get("conf", 0):
                    found[same] = x
        found.sort(key=lambda x: (x["bbox"][1] // 20, x["bbox"][0]))
        for x in found:
            x["text"] = x["text"].strip()
        return [x for x in found if re.search(r"\w", x["text"]) and not OCR_JUNK.search(x["text"])]

    def alike(a, b):
        return SequenceMatcher(None, a.lower(), b.lower()).ratio() > 0.6

    def same_line(a, b):
        a, b = a.lower(), b.lower()
        return SequenceMatcher(None, a, b).ratio() > 0.8 or (min(len(a), len(b)) >= 8 and (a in b or b in a))

    def mark_still(reads, end):
        """Flags (`x["c"]["still"]`) the lines that stay put for long: text of the page behind the subtitles. A line
        the caption box covers now and then is still the same line."""
        live, every = [], []
        for n, (t, boxes) in enumerate(reads):
            dt = (reads[n + 1][0] if n + 1 < len(reads) else end) - t
            live = [c for c in live if t - c["end"] <= STILL_GAP]
            for x in boxes:
                c = next((c for c in live if overlap(c["bbox"], x["bbox"]) > 0.5 and same_line(c["text"], x["text"])), None)
                if c is None:
                    c = {"bbox": x["bbox"], "text": x["text"], "t0": t, "end": t, "on": 0.0, "still": False}
                    live.append(c)
                    every.append(c)
                if len(x["text"]) > len(c["text"]):
                    c["text"], c["bbox"] = x["text"], x["bbox"]  # the whole line, seen while no caption covered it
                c["end"], c["on"] = t + dt, c["on"] + dt
                c["still"] = c["end"] - c["t0"] >= STILL_SPAN and c["on"] >= STILL_SHARE * (c["end"] - c["t0"])
                x["c"] = c
        return [c for c in every if c["still"]]

    def lines(boxes):
        """Boxes of one row that nearly touch are one line (OCR splits a long line in pieces)."""
        out = []
        for x in sorted(boxes, key=lambda x: x["bbox"][0]):
            x0, y0, x1, y1 = x["bbox"]
            l = next((l for l in out if min(y1, l["y1"]) - max(y0, l["y0"]) > 0.5 * min(y1 - y0, l["y1"] - l["y0"])
                      and x0 - l["x1"] <= 2.5 * (y1 - y0)), None)
            if l:
                l.update(text=l["text"] + " " + x["text"], x1=max(l["x1"], x1), y0=min(l["y0"], y0), y1=max(l["y1"], y1))
            else:
                out.append({"text": x["text"], "x0": x0, "x1": x1, "y0": y0, "y1": y1})
        return sorted(out, key=lambda l: l["y0"])

    def blocks(ls):
        """Lines one under the other, overlapping sideways (a caption of 2-3 lines, left- or centre-aligned in its
        box), form a block."""
        out = []
        for l in ls:
            prev = out[-1][-1] if out else None
            if prev and l["y0"] - prev["y1"] <= 0.8 * (l["y1"] - l["y0"]) \
                    and min(l["x1"], prev["x1"]) - max(l["x0"], prev["x0"]) >= 0.5 * min(l["x1"] - l["x0"], prev["x1"] - prev["x0"]):
                out[-1].append(l)
            else:
                out.append([l])
        return out

    def strip_still(text, pages):
        """Cuts a start or end that only continues page text the caption box hides (a prefix or suffix of it);
        the rest of a word it broke goes too."""
        low = text.lower()
        for s in pages:
            k = len(os.path.commonprefix([low, s]))
            if 3 <= k < len(text) and (k >= 8 or text[k].isspace()):
                return strip_still(re.sub(r"^\S*\s*", "", text[k:]) if not text[k].isspace() else text[k:].strip(), pages)
            j = len(os.path.commonprefix([low[::-1], s[::-1]]))
            if 3 <= j < len(text) and (j >= 8 or text[-j - 1].isspace()):
                cut = text[:-j]
                return strip_still(re.sub(r"\s*\S*$", "", cut) if not cut[-1:].isspace() else cut.strip(), pages)
        return text

    def trim(x, pages):
        """The box without a scrap of page text at its start or end; the box shrinks with the text."""
        text = strip_still(x["text"], pages)
        if text == x["text"] or not text:
            return x if text else None
        x0, y0, x1, y1 = x["bbox"]
        i = x["text"].find(text)
        i = max(i, 0)
        w = (x1 - x0) / len(x["text"])
        return {**x, "text": text, "bbox": [x0 + w * i, y0, x0 + w * (i + len(text)), y1]}

    def build(reads, end, stills, strict):
        cues, cur = [], None
        for t, boxes in reads:
            parts = []
            # Page text, or a scrap of it left visible beside the caption box.
            page = [c for c in stills if c["t0"] <= t <= c["end"] + STILL_GAP]
            bits = []
            for x in boxes:
                if x["c"]["still"] or any(overlap(x["bbox"], c["bbox"]) > 0.8 and x["text"].lower() in c["text"].lower() for c in page):
                    continue
                x = trim(x, [c["text"].lower() for c in page if min(x["bbox"][3], c["bbox"][3]) - max(x["bbox"][1], c["bbox"][1]) > 0])
                if x:
                    bits.append(x)
            for blk in blocks(lines(bits)):
                if strict and abs((min(l["x0"] for l in blk) + max(l["x1"] for l in blk)) / 2 - ow / 2) > ow * CENTER:
                    continue
                parts += [l["text"] for l in blk]
            text = " ".join(p for p in parts if p)
            if cur and text and alike(text, cur[1]):
                if len(text) > len(cur[1]):
                    cur = (cur[0], text)  # the fuller reading of the same line
                continue
            if cur:
                cues.append((cur[0], t, cur[1]))
            cur = (t, text) if text else None
        if cur:
            cues.append((cur[0], end, cur[1]))
        merged = []
        for c in (c for c in cues if c[1] - c[0] >= 0.7):
            if merged and c[0] - merged[-1][1] <= 2.0 and alike(c[2], merged[-1][2]):
                merged[-1] = (merged[-1][0], c[1], merged[-1][2])
            else:
                merged.append(c)
        return merged

    reads, last, since, i = [], None, 0, 0
    total = max(1, int(duration * 2))
    while True:
        frame = p.stdout.read(size)
        if len(frame) < size:
            break
        t = i / 2
        i += 1
        g = np.frombuffer(frame, np.uint8).reshape(oh, ow, 3)[:, :cols * step].reshape(oh, cols, step, 3).mean(axis=(2, 3))
        since += 1
        if last is not None and since < RECHECK:
            d = np.abs(g - last)
            # A small caption over a still picture moves the mean little but one cell a lot.
            r, c = oh // 8, cols // 8
            if d.mean() < DIFF_HI and d[:r * 8, :c * 8].reshape(r, 8, c, 8).mean(axis=(1, 3)).max() < DIFF_CELL:
                continue
        last, since = g, 0
        reads.append((t, read(frame)))
        if i % 120 == 0:
            on_progress(min(1.0, i / total))
    p.wait()
    stills = mark_still(reads, i / 2)
    # Left-aligned subtitles are rare: fall back to any position when nothing is centred.
    merged = build(reads, i / 2, stills, True) or build(reads, i / 2, stills, False)
    if not merged:
        raise JobError(f"OCR found no {lang} subtitles in the bottom {int((crop or 0.35) * 100)} % of the picture")
    return merged


# ---------- one job: run its pipeline graph ----------

STAGE = {"whisper": 1, "ocr": 1, "subtitle": 1, "translate": 2, "voice": 3}


def process(job):
    jid = job["job_id"]
    base = FILMSUB
    subs, duration = job.get("subs") or {}, job.get("duration") or 0
    g = job["pipeline"]
    nodes = {n["id"]: n for n in g["nodes"]}
    feeds = {}
    for a, b in g["edges"]:
        feeds.setdefault(b, []).append(a)
    order, seen = [], set()

    def visit(nid):
        if nid not in seen:
            seen.add(nid)
            for a in feeds.get(nid, []):
                visit(a)
            order.append(nid)

    for nid in nodes:
        visit(nid)
    tmp = tempfile.mkdtemp(prefix=f"narration-{jid}-", dir=WORK)
    # Progress bar: 1 subtitles 0-30 %, 2 translation 30-50 %, 3 voices 50-90 % (4, the mux, is Kellyfin's).
    span = {1: (0.0, 0.3), 2: (0.3, 0.2), 3: (0.5, 0.4)}
    count = {k: sum(1 for n in nodes.values() if STAGE.get(n["type"]) == k) for k in span}
    done = {k: 0 for k in span}

    def step(stage, label, frac=0.0):
        a, w = span[stage]
        part = (done[stage] + frac) / max(1, count[stage])
        try:
            kf_json("POST", f"/narrator/jobs/{jid}/progress", {"stage": stage, "step": label, "progress": a + w * min(1, part)})
        except requests.HTTPError as e:
            if e.response is not None and e.response.status_code == 410:
                raise JobCancelled() from e
        except Exception:
            pass

    @contextlib.contextmanager
    def exclusive(name, stage, waiting):
        """Holds one of the heavy services alone; while another video has it, this one says so (and reports, which
        keeps the job alive on the server and lets a deletion reach it)."""
        lock = RES[name]
        if not lock.acquire(blocking=False):
            step(stage, waiting)
            while not lock.acquire(timeout=20):
                step(stage, waiting)
        try:
            yield
        finally:
            lock.release()

    @retried
    def existing(lang):
        r = kf.get(kf_url(f"/narrator/jobs/{jid}/subtitle/{subs[lang]}"), timeout=300)
        r.raise_for_status()
        return parse_srt(r.text)

    uploaded = set()

    def keep(lang, cues):
        """Subtitles made here go next to the video (Kellyfin never overwrites the video's own)."""
        if lang not in subs and lang not in uploaded:
            path = os.path.join(tmp, f"{lang}.srt")
            write_srt(cues, path)
            kf_upload(jid, f"{lang}.srt", path)
            uploaded.add(lang)

    files = {}

    def fetch(kind):
        if kind not in files:
            files[kind] = os.path.join(tmp, "original.mka" if kind == "audio" else "video")
            step(1, "Fetching the original sound" if kind == "audio" else "Fetching the video (for OCR)")
            kf_download(f"/narrator/jobs/{jid}/{kind}", files[kind])
        return files[kind]

    texts = {}  # node id -> (lang, cues), made on demand: a source no output needs is never run

    def produce(nid):
        if nid in texts:
            return texts[nid]
        n, kind = nodes[nid], nodes[nid]["type"]
        lang = n.get("lang") or ""
        if kind == "subtitle":
            if lang not in subs:
                raise JobError(f"the video has no {lang} subtitles")
            step(1, f"Using the video's {lang} subtitles")
            texts[nid] = (lang, existing(lang))
        elif kind in ("whisper", "ocr") and lang != "auto" and lang in subs:
            step(1, f"Using the video's {lang} subtitles")
            texts[nid] = (lang, existing(lang))
        elif kind == "whisper":
            audio = fetch("audio")
            step(1, f"Transcribing {'the spoken language' if lang == 'auto' else lang} (Whisper)")
            with exclusive("whisper", 1, "Waiting for Whisper (another video is being transcribed)"):
                step(1, f"Transcribing {'the spoken language' if lang == 'auto' else lang} (Whisper)")
                cues = whisper(base, audio, lang, tmp)
            if lang == "auto":
                lang = detect(base, cues)
                log(f"job {jid}: spoken language {lang}")
            texts[nid] = (lang, cues)
            keep(lang, cues)
        elif kind == "ocr":
            video = fetch("video")
            langs = [lang] + [l for l in n.get("also") or [] if l != lang]
            label = f"Reading the burned-in {'+'.join(langs)} subtitles (OCR)"
            step(1, label)
            with exclusive("ocr", 1, "Waiting for OCR (another video is being read)"):
                step(1, label)
                cues = ocr(base, video, langs, n.get("crop") or 0.35, duration, lambda f, label=label: step(1, label, f))
            log(f"job {jid}: OCR found {len(cues)} {lang} lines")
            texts[nid] = (lang, cues)
            keep(lang, cues)
        elif kind == "translate":
            if lang in subs:  # the video has it: its input is not needed
                step(2, f"Using the video's {lang} subtitles")
                texts[nid] = (lang, existing(lang))
            else:
                src, cues = produce(feeds[nid][0])
                if lang == src:
                    texts[nid] = (lang, cues)
                else:
                    label = f"Translating {src} → {lang} (Argos)"
                    step(2, label)
                    with exclusive("translate", 2, "Waiting for translation (another video is being translated)"):
                        step(2, label)
                        texts[nid] = (lang, translate(base, cues, src, lang, lambda f, label=label: step(2, label, f)))
                    keep(lang, texts[nid][1])
        if kind in STAGE:
            done[STAGE[kind]] += 1
        return texts[nid]

    try:
        log(f"job {jid}: {job['title']} -> pipeline of {len(nodes)} nodes via {base}")
        # Outputs: every translation and every source wired to nothing (a subtitle file); voices pull what they read.
        used = {a for a, _ in g["edges"]}
        for nid in order:
            if nodes[nid]["type"] == "translate" or (nodes[nid]["type"] in ("whisper", "ocr") and nid not in used):
                produce(nid)
        # The voices (say Vietnamese and Chinese) are read aloud at the same time.
        plan = []
        for nid in order:
            n = nodes[nid]
            if n["type"] != "voice":
                continue
            lang, cues = produce(feeds[nid][0])
            cues = [c for c in cues if speakable(c[2])]
            if not cues:
                raise JobError(f"no {lang} lines to read out")
            if not duration:  # unprobed video: the timeline ends with the last line
                duration = max(c[1] for c in cues) + 1
            plan.append((n, lang, cues))

        def speak(n, lang, cues):
            wav = os.path.join(tmp, f"{lang}.voice.wav")
            if n.get("engine") == "vieneu":
                label = "Reading vi aloud (VieNeu)"
                srt = os.path.join(tmp, f"{lang}.voice.srt")
                write_srt(cues, srt)
                with exclusive("vieneu", 3, "Waiting for VieNeu (another video is being read)"):
                    step(3, label)
                    tts_vi(base, srt, n.get("voice") or job["voice_vi"], wav, lambda f, label=label: step(3, label, f))
            else:
                voice = n.get("voice") or (job["voice_zh"] if lang == "zh" else EDGE_VOICES.get(lang))
                if not voice:
                    raise JobError(f"no edge-tts voice for {lang}: set one on the voice node")
                label = f"Reading {lang} aloud (edge-tts {voice})"
                step(3, label)
                tts_edge(cues, voice, duration, wav, tmp, lambda f, label=label: step(3, label, f))
            flac = os.path.join(tmp, f"{lang}.voice.flac")
            to_flac(wav, flac)
            kf_upload(jid, f"{lang}.voice.flac", flac)
            log(f"job {jid}: {lang} voice uploaded ({os.path.getsize(flac) >> 20} MB)")
            done[3] += 1

        with ThreadPoolExecutor(max(1, len(plan))) as pool:
            for f in [pool.submit(speak, *p) for p in plan]:
                f.result()
        kf_json("POST", f"/narrator/jobs/{jid}/done")
        log(f"job {jid}: handed to Kellyfin for the mux")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


# ---------- service report sent with every poll ----------

_report = {"at": 0.0, "edge_at": 0.0, "checks": [], "voices": [], "edge": None, "edge_voices": []}


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

    def ocr_ok():
        r = requests.get(f"{FILMSUB}/ocr/health", timeout=10)
        r.raise_for_status()
        return True, r.text.strip()

    voices = []

    def vieneu_ok():
        requests.get(f"{FILMSUB}/tts/", timeout=8).raise_for_status()  # not installed: answer in seconds, not minutes
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
            try:
                _report["edge_voices"] = sorted(v["ShortName"] for v in asyncio.run(asyncio.wait_for(edge_tts.list_voices(), 20)))
            except Exception:
                pass
        return _report["edge"], "Microsoft speech service reachable" if _report["edge"] else "no audio"

    checks = [probe("Whisper (subtitles)", whisper_ok), probe("EasyOCR (burned-in subtitles)", ocr_ok),
              probe("Argos (translation)", argos_ok),
              probe("VieNeu (Vietnamese voice)", vieneu_ok), probe("edge-tts (Chinese voice, Internet)", edge_ok)]
    _report.update(at=now, checks=checks, voices=voices)
    return checks, voices


def run_job(job):
    try:
        process(job)
    except JobCancelled:
        log(f"job {job['job_id']}: deleted or paused in Settings, stopped")
    except Exception as e:
        msg = str(e) if isinstance(e, JobError) else f"{type(e).__name__}: {e}"
        log(f"job {job['job_id']}: failed: {msg}")
        try:
            kf_json("POST", f"/narrator/jobs/{job['job_id']}/fail", {"error": msg})
        except Exception:
            pass


def main():
    log(f"worker {NAME} -> {SERVER}, film-sub {FILMSUB}, up to {WORKER_JOBS} videos at once")
    running = {}  # job id -> thread
    while True:
        running = {i: t for i, t in running.items() if t.is_alive()}
        # While videos are in work the services are busy: the last check stands (a probe could time out behind a job).
        checks, voices = (_report["checks"], _report["voices"]) if running and _report["checks"] else health()
        try:
            r = kf.post(kf_url("/narrator/claim"), json={"worker": NAME, "checks": checks, "voices": voices,
                                                         "edge_voices": _report["edge_voices"], "active": list(running),
                                                         "take": len(running) < WORKER_JOBS}, timeout=60)
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
            time.sleep(5 if running else POLL)
            continue
        job = r.json()
        t = threading.Thread(target=run_job, args=(job,), daemon=True)
        t.start()
        running[job["job_id"]] = t  # and the next one straight away, until every slot is taken


if __name__ == "__main__":
    main()
