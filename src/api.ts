import { createSignal } from "solid-js";

export type User = { id: number; name: string; is_admin: boolean };
export type Card = {
  id: number;
  library_id: number;
  title: string;
  duration: number | null;
  width: number | null;
  height: number | null;
  img: string | null;
  imgv: number | null;
  color: string | null;
  position: number;
  played: boolean;
};
export type Lib = { id: number; name: string; kind: string; count: number; img: string | null; imgv: number | null; color: string | null };
export type LatestRow = { id: number; name: string; items: Card[] };
export type Home = { libraries: Lib[]; resume: Card[]; latest: LatestRow[] };
export type Stream = {
  index: number;
  type: string;
  codec: string | null;
  lang: string | null;
  title: string | null;
  default: boolean;
  channels: number | null;
};
export type SubTrack = { key: string; lang: string | null; title: string | null; codec: string; external: boolean; supported: boolean };
export type ItemDetail = Card & {
  library_name: string;
  path: string;
  size: number;
  added_at: number;
  container: string | null;
  video_codec: string | null;
  bitrate: number | null;
  streams: Stream[] | null;
  favorite: boolean;
  ext: string;
  subtitles: SubTrack[];
  index_size: number | null;
  prev_id: number | null;
  next_id: number | null;
};
export type ScanStatus = { running: boolean; phase: string; done: number; total: number; error: string | null; finished_at: number | null };

export const isTauri = "__TAURI_INTERNALS__" in window;

function load(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function save(key: string, v: string | null) {
  try {
    v === null ? localStorage.removeItem(key) : localStorage.setItem(key, v);
  } catch {}
}

// Web build is served by the server itself; native apps must be told where it is.
const [server, setServerSignal] = createSignal(load("server") ?? (isTauri ? "" : location.origin));
export { server };
export function setServer(url: string) {
  const u = url.trim().replace(/\/+$/, "");
  save("server", u);
  setServerSignal(u);
}

let token = load("token");
export const authToken = () => token;
const [user, setUser] = createSignal<User | null>(null);
export { user };

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (token) headers.Authorization = `Bearer ${token}`;
  const r = await fetch(`${server()}/api${path}`, { ...init, headers });
  if (r.status === 401 && token) dropSession();
  if (!r.ok) {
    const body = await r.json().catch(() => ({}));
    throw new ApiError(r.status, body.error ?? r.statusText);
  }
  return r.status === 204 || r.status === 202 ? (undefined as T) : r.json();
}

function dropSession() {
  token = null;
  save("token", null);
  setUser(null);
}

export async function login(username: string, password: string) {
  const r = await api<{ token: string; user: User }>("/auth/login", {
    method: "POST",
    body: JSON.stringify({ username, password }),
  });
  token = r.token;
  save("token", r.token);
  setUser(r.user);
}

export async function logout() {
  await api("/auth/logout", { method: "POST" }).catch(() => {});
  dropSession();
}

/** Resolves the stored session; false when a login is needed. */
export async function restore(): Promise<boolean> {
  if (!token || !server()) return false;
  try {
    setUser(await api<User>("/me"));
    return true;
  } catch {
    return false;
  }
}

export type Size = "s" | "m" | "l";
export const img = (c: { img: string | null; imgv: number | null }, size: Size) =>
  c.img ? `${server()}${c.img}${size}.webp?v=${c.imgv}` : undefined;

export function fmtDuration(sec: number | null) {
  if (!sec) return "";
  const m = Math.round(sec / 60);
  return m >= 60 ? `${Math.floor(m / 60)}h ${m % 60}m` : `${m}m`;
}

/** URL for <video>/<track> use: media elements cannot send headers, so the token rides in ?t=. */
export const mediaUrl = (path: string, q: Record<string, string | number> = {}) =>
  `${server()}/api${path}?${new URLSearchParams({ ...q, t: token ?? "" } as Record<string, string>)}`;

export function fmtClock(sec: number) {
  const s = Math.max(0, Math.floor(sec));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
  const mm = h ? String(m).padStart(2, "0") : String(m);
  return `${h ? h + ":" : ""}${mm}:${String(r).padStart(2, "0")}`;
}

const langNames = (() => {
  try {
    return new Intl.DisplayNames(["en"], { type: "language" });
  } catch {
    return null;
  }
})();
export function langName(code: string | null) {
  if (!code) return null;
  try {
    return langNames?.of(code) ?? code;
  } catch {
    return code;
  }
}

export const streamLabel = (s: Stream) =>
  [s.title, langName(s.lang && s.lang !== "und" ? s.lang : null), s.codec?.toUpperCase(), s.channels ? `${s.channels}ch` : null]
    .filter(Boolean)
    .join(" · ");

export const subLabel = (s: SubTrack) =>
  [s.title ?? langName(s.lang) ?? "Unknown", s.external ? "External" : s.codec.toUpperCase(), s.supported ? null : "unsupported"]
    .filter(Boolean)
    .join(" · ");

export function fmtRes(h: number | null, w: number | null) {
  if (!h || !w) return "";
  const p = Math.min(h, w);
  return p >= 2000 ? "4K" : `${p}p`;
}
