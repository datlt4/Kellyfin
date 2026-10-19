// App updates (not the web build): desktop installs the signed release in place, Android opens the APK
// download, iOS only reports it (the Mac's re-sign job installs it).
import { createSignal } from "solid-js";
import { isTauri } from "./api";

export type UpdateInfo = { version: string; notes: string; action: "install" | "download" | "none"; url: string };

const [available, setAvailable] = createSignal<UpdateInfo | null>(null);
const [progress, setProgress] = createSignal<number | null>(null); // 0..1 while installing
const [dismissed, setDismissed] = createSignal(false);
export { available as updateAvailable, progress as updateProgress, dismissed as updateDismissed, setDismissed as dismissUpdate };

const core = () => import("@tauri-apps/api/core");

export async function checkUpdate(): Promise<UpdateInfo | null> {
  if (!isTauri) return null;
  const { invoke } = await core();
  const u = await invoke<UpdateInfo | null>("update_check");
  setAvailable(u);
  return u;
}

export async function applyUpdate() {
  const u = available();
  if (!u) return;
  const { invoke, Channel } = await core();
  if (u.action === "install") {
    const ch = new Channel<[number, number]>();
    ch.onmessage = ([done, total]) => setProgress(total ? done / total : 0);
    setProgress(0);
    try {
      await invoke("update_install", { onProgress: ch }); // restarts the app when done
    } finally {
      setProgress(null);
    }
  } else if (u.action === "download") {
    await invoke("open_release", { url: u.url });
  }
}

/** Looks once per app start, a few seconds in so it does not compete with the first page. */
export function autoCheck() {
  if (isTauri) setTimeout(() => checkUpdate().catch(() => {}), 5000);
}
