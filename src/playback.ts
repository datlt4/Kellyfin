import type { ItemDetail, Stream, SubTrack } from "./api";

export type Mode = "direct" | "remux" | "hls";
/** "orig": the file itself (direct/remux); "auto": adaptive transcode; number: fixed transcode height. */
export type Quality = "orig" | "auto" | number | `ai:${string}`;

// A bigger MP4 index delays the first frame by seconds over VPN; a copy-remux starts at once.
const MAX_DIRECT_INDEX = 4 * 1024 * 1024;

const VIDEO: Record<string, string> = {
  h264: "avc1.640028",
  hevc: "hvc1.1.6.L120.90",
  av1: "av01.0.08M.08",
  vp9: "vp09.00.40.08",
  vp8: "vp8",
};
const AUDIO: Record<string, string> = { aac: "mp4a.40.2", mp3: "mp4a.69", opus: "opus", vorbis: "vorbis", flac: "flac" };

const probe = document.createElement("video");

function canPlay(container: "mp4" | "webm", video: string | null, audio: string | null | undefined) {
  const v = video ? VIDEO[video] : undefined;
  const a = audio ? AUDIO[audio] : undefined;
  if (!v || (audio && !a)) return false;
  return probe.canPlayType(`video/${container}; codecs="${[v, a].filter(Boolean).join(", ")}"`) !== "";
}

/** Whether the remux can pass this audio through instead of re-encoding to AAC. */
export function canCopyAudio(it: ItemDetail, audio: number | null) {
  const a = audioStreams(it).find((s) => s.index === audio) ?? nativeAudio(it);
  return !!a && canPlay("mp4", it.video_codec, a.codec);
}

// WebKit on Apple (Safari, every iOS browser, Tauri on iOS/macOS).
const appleWebKit = /AppleWebKit/.test(navigator.userAgent) && !/Chrome\/|Android/.test(navigator.userAgent);

export const audioStreams = (it: ItemDetail): Stream[] => it.streams?.filter((s) => s.type === "audio") ?? [];

/** The track a browser plays from the raw file: flagged default, else the first. */
export const nativeAudio = (it: ItemDetail) => {
  const a = audioStreams(it);
  return a.find((s) => s.default) ?? a[0];
};

/**
 * Direct play when the browser decodes the file as-is with the wanted audio track;
 * otherwise copy-remux on the server (cheap) when the video codec itself is playable; else null (transcode).
 */
export function originalMode(it: ItemDetail, audio: number | null): "direct" | "remux" | null {
  const a = audioStreams(it).find((s) => s.index === audio) ?? nativeAudio(it);
  const container = ["mp4", "m4v", "mov"].includes(it.ext) ? "mp4" : it.ext === "webm" ? "webm" : null;
  const smallIndex = (it.index_size ?? 0) <= MAX_DIRECT_INDEX;
  const tag = it.streams?.find((s) => s.type === "video")?.tag;
  // WebKit plays hev1-tagged HEVC as audio over a black picture; only hvc1 shows.
  const tagOk = !(appleWebKit && it.video_codec === "hevc" && tag !== "hvc1");
  if (container && smallIndex && tagOk && a?.index === nativeAudio(it)?.index && canPlay(container, it.video_codec, a?.codec)) return "direct";
  // Remux streams without byte ranges, which WebKit refuses for <video>; it gets a transcode instead.
  // Remux output is MP4 with AAC audio.
  if (!appleWebKit && canPlay("mp4", it.video_codec, a ? "aac" : null)) return "remux";
  return null;
}

/** Same rungs as the server ladder: the source size (max 1080) and every standard size below it. */
export function qualities(it: ItemDetail): number[] {
  const short = Math.min(it.width ?? 0, it.height ?? 0, 1080);
  if (!short) return [];
  return [short, ...[1080, 720, 480, 360, 240, 144].filter((p) => p < short)];
}

// Mirrors the server ladder (video kbps) plus 160k audio.
const KBPS: [number, number][] = [[1080, 5000], [720, 2800], [480, 1400], [360, 800], [240, 450], [144, 250]];
const kbpsFor = (p: number) => ([...KBPS].reverse().find(([lp]) => lp >= p)?.[1] ?? 5000) + 160;

/** Highest rung that fits the screen and the measured bandwidth (with headroom); the lowest one otherwise. */
export function autoQuality(rungs: number[], bps: number, screenP: number): number {
  return rungs.find((p) => p <= screenP * 1.1 && kbpsFor(p) * 1000 * 1.4 <= bps) ?? rungs[rungs.length - 1];
}

/** Short side of the player in device pixels. */
export const screenP = (el: HTMLElement) =>
  Math.min(el.clientHeight, (el.clientWidth * 9) / 16) * (window.devicePixelRatio || 1);

let measured: { bps: number; at: number } | null = null;
/** Throughput to the server, measured with a 2 MB download and reused for 10 minutes. */
export async function bandwidth(base: string, token: string): Promise<number> {
  if (measured && Date.now() - measured.at < 600_000) return measured.bps;
  const bytes = 2_000_000;
  const t0 = performance.now();
  try {
    const r = await fetch(`${base}/api/speedtest?bytes=${bytes}`, { headers: { Authorization: `Bearer ${token}` } });
    await r.arrayBuffer();
  } catch {
    return 3_000_000;
  }
  const bps = (bytes * 8) / ((performance.now() - t0) / 1000);
  measured = { bps, at: Date.now() };
  return bps;
}

export const nativeHls = () => probe.canPlayType("application/vnd.apple.mpegurl") !== "";
export const mseHls = () => "MediaSource" in window || "ManagedMediaSource" in window;

export function preferredQuality(): Quality {
  const q = pref("quality");
  return q === "auto" ? "auto" : q && /^\d+$/.test(q) ? Number(q) : "orig";
}

const pref = (key: string) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};
export const remember = (key: string, v: string) => {
  try {
    localStorage.setItem(key, v);
  } catch {}
};

const sameLang = (a: string | null | undefined, b: string | null | undefined) =>
  !!a && !!b && (a === b || a.slice(0, 2) === b.slice(0, 2));

/** Last audio language the user picked, else the file's default. */
export function defaultAudio(it: ItemDetail): number | null {
  const want = pref("audioLang");
  return (audioStreams(it).find((s) => sameLang(s.lang, want)) ?? nativeAudio(it))?.index ?? null;
}

/** Last subtitle language the user picked ("off" sticks too), else Vietnamese, else off. */
export function defaultSub(it: ItemDetail): string {
  const want = pref("subLang") ?? "vi";
  if (want === "off") return "off";
  const ok = it.subtitles.filter((s) => s.supported);
  return ok.find((s) => sameLang(s.lang, want))?.key ?? "off";
}

export const subLang = (s: SubTrack | undefined) => s?.lang ?? "off";

/** Second subtitle shown above the first (language learning): the last one picked, never the same track. */
export function defaultSub2(it: ItemDetail, primary: string): string {
  const want = pref("sub2Lang");
  if (!want || want === "off") return "off";
  return it.subtitles.find((s) => s.supported && s.key !== primary && sameLang(s.lang, want))?.key ?? "off";
}

export type Cue = { start: number; end: number; text: string };

function ts(s: string) {
  const p = s.trim().split(":").map(Number);
  return p.reduce((acc, x) => acc * 60 + x, 0);
}

/** Minimal WebVTT parser; markup is stripped (cues render as plain text). */
export function parseVtt(src: string): Cue[] {
  const cues: Cue[] = [];
  for (const block of src.replace(/\r/g, "").split(/\n\n+/)) {
    const lines = block.split("\n");
    const i = lines.findIndex((l) => l.includes("-->"));
    if (i < 0) continue;
    const [a, b] = lines[i].split("-->");
    const text = lines
      .slice(i + 1)
      .join("\n")
      .replace(/<[^>]*>/g, "")
      .trim();
    if (text) cues.push({ start: ts(a), end: ts(b.trim().split(/\s+/)[0]), text });
  }
  return cues.sort((x, y) => x.start - y.start);
}

/** Text of the cues active at t (binary search on start, cues may overlap). */
export function cueAt(cues: Cue[], t: number): string {
  let lo = 0, hi = cues.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (cues[mid].start <= t) lo = mid + 1;
    else hi = mid;
  }
  const out: string[] = [];
  for (let i = lo - 1; i >= 0 && i >= lo - 8; i--) if (cues[i].end > t) out.unshift(cues[i].text);
  return out.join("\n");
}
