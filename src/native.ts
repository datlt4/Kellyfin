// Device controls a web page cannot reach. The Android app injects window.KellyfinNative (MainActivity.kt).

type Bridge = {
  setFullscreen(on: boolean): void;
};

const android = (window as unknown as { KellyfinNative?: Bridge }).KellyfinNative;
/** Fullscreen with hidden system bars and landscape even when auto-rotate is off (Android app). */
export const nativeFullscreen = !!android;

export const setFullscreen = (on: boolean) => android?.setFullscreen(on);

// The Android app already pads the WebView by the system bars: the page must not add CSS safe areas on top.
if (android) {
  document.documentElement.style.setProperty("--safe-top", "0px");
  document.documentElement.style.setProperty("--safe-bottom", "0px");
}
