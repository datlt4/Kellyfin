// Device controls a web page cannot reach. The Android app injects window.KellyfinNative (MainActivity.kt).
import { isTauri } from "./api";

type Bridge = {
  setFullscreen(on: boolean): void;
  setPlaying?(on: boolean): void; // absent in apps older than 2.1.0
};

const android = (window as unknown as { KellyfinNative?: Bridge }).KellyfinNative;
// WebKitGTK 2.50 aborts the whole Linux app on element fullscreen: the window goes fullscreen instead.
const linuxApp = isTauri && /Linux/.test(navigator.userAgent) && !android;

/** Android app: hidden system bars and landscape even with auto-rotate off. Linux app: a fullscreen window. */
export const nativeFullscreen = !!android || linuxApp;

export function setFullscreen(on: boolean) {
  if (android) android.setFullscreen(on);
  else if (linuxApp) import("@tauri-apps/api/window").then((w) => w.getCurrentWindow().setFullscreen(on)).catch(() => {});
}

/** Android app: keep the sound when the app goes to the background or the screen locks. */
export const setPlaying = (on: boolean) => android?.setPlaying?.(on);

// The Android app already pads the WebView by the system bars: the page must not add CSS safe areas on top.
if (android) {
  document.documentElement.style.setProperty("--safe-top", "0px");
  document.documentElement.style.setProperty("--safe-bottom", "0px");
}
