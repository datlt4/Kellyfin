// Device controls a web page cannot reach. The Android app injects window.KellyfinNative
// (MainActivity.kt); the iOS app exposes Tauri commands. Elsewhere these fall back to web APIs.
import { isTauri } from "./api";

type Bridge = {
  getBrightness(): number;
  setBrightness(v: number): void; // < 0: back to the system setting
  getVolume(): number;
  setVolume(v: number): void;
  setFullscreen(on: boolean): void;
};

const android = (window as unknown as { KellyfinNative?: Bridge }).KellyfinNative;
const ios = isTauri && /iPhone|iPad/.test(navigator.userAgent);

/** Screen brightness of the device (Android, iOS apps) rather than a filter on the video. */
export const nativeBrightness = !!android || ios;
/** Media volume of the phone (Android app). iOS forbids setting it: only the hardware buttons can. */
export const nativeVolume = !!android;
/** Fullscreen with hidden system bars and landscape even when auto-rotate is off (Android app). */
export const nativeFullscreen = !!android;

const invoke = async <T,>(cmd: string, args?: Record<string, unknown>) =>
  (await import("@tauri-apps/api/core")).invoke<T>(cmd, args);

export async function getBrightness(): Promise<number> {
  if (android) return android.getBrightness();
  if (ios) return invoke<number>("get_brightness").catch(() => 1);
  return 1;
}

export function setBrightness(v: number) {
  if (android) android.setBrightness(v);
  else if (ios && v >= 0) invoke("set_brightness", { value: v }).catch(() => {});
}

export const getVolume = () => android?.getVolume() ?? 1;
export const setVolume = (v: number) => android?.setVolume(v);
export const setFullscreen = (on: boolean) => android?.setFullscreen(on);

// The Android app already pads the WebView by the system bars: the page must not add CSS safe areas on top.
if (android) {
  document.documentElement.style.setProperty("--safe-top", "0px");
  document.documentElement.style.setProperty("--safe-bottom", "0px");
}
