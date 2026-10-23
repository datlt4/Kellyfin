import { createSignal } from "solid-js";

export type User = { id: number; name: string; is_admin: boolean; img?: string | null; imgv?: number | null };
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
  tag?: string | null;
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
  /** Picture without black bars, "w:h:x:y" in source pixels; "" or null: none. */
  crop: string | null;
  streams: Stream[] | null;
  favorite: boolean;
  ext: string;
  subtitles: SubTrack[];
  index_size: number | null;
  upscaled: { model: string; width: number | null; height: number | null }[];
  prev_id: number | null;
  next_id: number | null;
};
export type Related = { libraries: Lib[]; items: Card[] };
export type Session = { user: string; item_id: number; title: string; position: number; duration: number | null; mode: string };
export type JobInfo = { item_id: number; p: number; audio: number; encoder: string; position: number; paused: boolean };
export type Info = {
  version: string;
  libraries: number;
  items: number;
  users: number;
  hw_device: string | null;
  in_docker: boolean;
  sessions: Session[];
  transcodes: JobInfo[];
};
export type UserRow = { id: number; name: string; is_admin: boolean; has_password: boolean; created_at: number };
export type LibRow = { id: number; name: string; kind: string; paths: string[]; count: number; has_image: boolean };
export type FsList = { path: string; parent: string | null; dirs: string[] };
export type UpscaleModel = { name: string; description: string };
export type UpscaleOverview = {
  token: string;
  workers: { name: string; models: UpscaleModel[]; seen_secs_ago: number }[];
  jobs: {
    id: number; item_id: number; title: string; library: string; model: string; status: string;
    progress: number; error: string | null; worker: string | null; height: number | null; updated_at: number;
  }[];
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

/** A saved server in the native apps: plain http(s), or behind a proxy that wants extra headers (Pangolin). */
export type ServerProfile = { id: string; name: string; url: string; headers: [string, string][] };

/** In a browser, the server that served the page is always the first entry. */
export const HERE = "here";
const hereProfile = (): ServerProfile => ({ id: HERE, name: `This server (${location.host})`, url: location.origin, headers: [] });

export function savedServers(): ServerProfile[] {
  let list: ServerProfile[] = [];
  try {
    list = JSON.parse(load("servers") ?? "[]");
  } catch {}
  return isTauri ? list : [hereProfile(), ...list.filter((s) => s.id !== HERE)];
}
export function saveServers(list: ServerProfile[]) {
  save("servers", JSON.stringify(list.filter((s) => s.id !== HERE)));
}

// Web build is served by the server itself; native apps pick a saved server.
const [server, setServerSignal] = createSignal(isTauri ? "" : location.origin);
const [activeServer, setActiveServer] = createSignal<ServerProfile | null>(null);
export { server, activeServer };

// One session per server ("token" for the page's own server, as before profiles existed).
const tokenKey = () => (!isTauri && (activeServer()?.id ?? HERE) === HERE ? "token" : `token:${activeServer()?.id ?? ""}`);
let token = isTauri ? null : load("token");
export const authToken = () => token;

/** Base URL the UI talks to: the server itself, or the app's local proxy when headers must be added. */
export const appVersion = __APP_VERSION__;
const [serverVersion, setServerVersion] = createSignal<string | null>(null);
export { serverVersion };

// Apps served from tauri:// (iOS, macOS, Linux) are a secure origin to WebKit, which blocks plain-http servers
// as mixed content: those go through the app's local proxy (127.0.0.1 is allowed). http://tauri.localhost apps
// (Android, Windows) reach them directly.
const secureApp = isTauri && location.protocol !== "http:";

async function baseFor(p: ServerProfile): Promise<string> {
  const url = p.url.trim().replace(/\/+$/, "");
  const headers = p.headers.filter(([k]) => k.trim());
  if (!headers.length && !(secureApp && url.startsWith("http://"))) return url;
  if (!isTauri) throw new Error("Servers that need headers only work in the Kellyfin app (browsers cannot add headers to video and images)");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string>("set_server", { url, headers });
}

export async function useServer(p: ServerProfile) {
  setServerSignal(await baseFor(p));
  setServerVersion(null);
  fetch(`${server()}/api/health`)
    .then((r) => r.json())
    .then((h) => server() && setServerVersion(h.version ?? null))
    .catch(() => {});
  setActiveServer(p);
  save("activeServer", p.id);
  token = load(tokenKey());
}

// iOS kills the local proxy's listener while the app is suspended; set_server then serves on a new base.
document.addEventListener("visibilitychange", async () => {
  const p = activeServer();
  if (document.hidden || !p || !server().startsWith("http://127.0.0.1")) return;
  if ((await baseFor(p).catch(() => server())) !== server()) location.reload();
});

/** Checks that a server answers as Kellyfin (headers included); returns its version. */
export async function probeServer(p: ServerProfile): Promise<string> {
  const r = await fetch(`${await baseFor(p)}/api/health`).catch((e) => {
    throw new Error(`Cannot reach server: ${e.message}`);
  });
  const body = await r.json().catch(() => null);
  if (!r.ok || body?.app !== "kellyfin") throw new Error(r.ok ? "Not a Kellyfin server (check URL and headers)" : `HTTP ${r.status}`);
  return body.version;
}
const [user, setUser] = createSignal<User | null>(null);
export { user };
/** Set when the saved server did not answer at start-up (e.g. a LAN address from outside): Login offers the others. */
const [unreachable, setUnreachable] = createSignal<string | null>(null);
export { unreachable };

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
  const text = await r.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

function dropSession() {
  token = null;
  save(tokenKey(), null);
  setUser(null);
}

/** Uploads a file as the raw request body (images). */
export async function upload(path: string, file: Blob, method = "PUT") {
  const r = await fetch(`${server()}/api${path}`, {
    method,
    body: file,
    headers: { "Content-Type": file.type || "application/octet-stream", Authorization: `Bearer ${token}` },
  });
  if (!r.ok) throw new ApiError(r.status, (await r.json().catch(() => ({}))).error ?? r.statusText);
}

/** Re-reads the signed-in user (after a profile change). */
export async function refreshUser() {
  setUser(await api<User>("/me"));
}

export async function login(username: string, password: string) {
  const r = await api<{ token: string; user: User }>("/auth/login", {
    method: "POST",
    body: JSON.stringify({ username, password }),
  });
  token = r.token;
  save(tokenKey(), r.token);
  setUser(r.user);
  await refreshUser().catch(() => {});
}

/** Back to the server picker; this server's session is kept for when it is picked again. */
export function switchServer() {
  setUser(null);
}

/** Moves to another saved server: straight in when it still has a session, else to its sign-in. */
export async function switchTo(p: ServerProfile) {
  setUser(null);
  await useServer(p);
  await restore();
}

export async function logout() {
  await api("/auth/logout", { method: "POST" }).catch(() => {});
  dropSession();
}

/** Resolves the stored session; false when a login is needed. */
export async function restore(): Promise<boolean> {
  const list = savedServers();
  const p = list.find((s) => s.id === load("activeServer")) ?? list[0];
  if (!p) return false;
  await useServer(p).catch(() => {});
  if (!token || !server()) return false;
  setUnreachable(null);
  // An unreachable address can hang for a minute or more: give up after 8 s.
  const stop = new AbortController();
  const timer = setTimeout(() => stop.abort(), 8000);
  try {
    setUser(await api<User>("/me", { signal: stop.signal }));
    return true;
  } catch (e) {
    if (!(e instanceof ApiError)) setUnreachable(`Cannot reach ${p.name}. Check the network or pick another server.`);
    return false;
  } finally {
    clearTimeout(timer);
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
