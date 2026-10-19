import type { ItemDetail, Stream, SubTrack } from "./api";

export type Mode = "direct" | "remux" | "unsupported";

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

export const audioStreams = (it: ItemDetail): Stream[] => it.streams?.filter((s) => s.type === "audio") ?? [];

/** The track a browser plays from the raw file: flagged default, else the first. */
export const nativeAudio = (it: ItemDetail) => {
  const a = audioStreams(it);
  return a.find((s) => s.default) ?? a[0];
};

/**
 * Direct play when the browser decodes the file as-is with the wanted audio track;
 * otherwise copy-remux on the server (cheap), since the video codec itself is playable.
 */
export function pickMode(it: ItemDetail, audio: number | null): Mode {
  const a = audioStreams(it).find((s) => s.index === audio) ?? nativeAudio(it);
  const container = ["mp4", "m4v", "mov"].includes(it.ext) ? "mp4" : it.ext === "webm" ? "webm" : null;
  const smallIndex = (it.index_size ?? 0) <= MAX_DIRECT_INDEX;
  if (container && smallIndex && a?.index === nativeAudio(it)?.index && canPlay(container, it.video_codec, a?.codec)) return "direct";
  // Remux output is MP4 with AAC audio.
  if (canPlay("mp4", it.video_codec, a ? "aac" : null)) return "remux";
  return "unsupported";
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
