// Chromecast through the Google Cast Web Sender SDK (Chrome only), plus AirPlay on WebKit.
import { createSignal } from "solid-js";
import { isTauri } from "./api";

/* eslint-disable @typescript-eslint/no-explicit-any */
declare global {
  interface Window {
    __onGCastApiAvailable?: (ok: boolean) => void;
    cast?: any;
    chrome?: any;
    WebKitPlaybackTargetAvailabilityEvent?: unknown;
  }
}

export type CastMedia = {
  url: string;
  contentType: string;
  title: string;
  subtitle?: string;
  image?: string;
  vtt?: string;
  start: number;
};

const [available, setAvailable] = createSignal(false);
const [device, setDevice] = createSignal<string | null>(null); // receiver name while connected
const [remoteTime, setRemoteTime] = createSignal(0);
const [remotePaused, setRemotePaused] = createSignal(true);
export { available as castAvailable, device as castDevice, remoteTime, remotePaused };

let player: any;
let controller: any;
let loaded = false;

/** Loads the sender SDK once; the cast button appears when a receiver could be used. */
export function initCast() {
  // The SDK talks to the Cast extension built into Chrome; app webviews and other browsers lack it.
  if (loaded || isTauri || !/Chrome\//.test(navigator.userAgent) || /Edg\//.test(navigator.userAgent)) return;
  loaded = true;
  window.__onGCastApiAvailable = (ok) => {
    if (!ok || !window.cast?.framework) return;
    const { cast, chrome } = window;
    const ctx = cast.framework.CastContext.getInstance();
    ctx.setOptions({
      receiverApplicationId: chrome.cast.media.DEFAULT_MEDIA_RECEIVER_APP_ID,
      autoJoinPolicy: chrome.cast.AutoJoinPolicy.ORIGIN_SCOPED,
    });
    player = new cast.framework.RemotePlayer();
    controller = new cast.framework.RemotePlayerController(player);
    const E = cast.framework.RemotePlayerEventType;
    controller.addEventListener(E.IS_CONNECTED_CHANGED, () => {
      setDevice(player.isConnected ? (ctx.getCurrentSession()?.getCastDevice()?.friendlyName ?? "TV") : null);
    });
    controller.addEventListener(E.CURRENT_TIME_CHANGED, () => setRemoteTime(player.currentTime));
    controller.addEventListener(E.IS_PAUSED_CHANGED, () => setRemotePaused(player.isPaused));
    setAvailable(true);
  };
  const s = document.createElement("script");
  s.src = "https://www.gstatic.com/cv/js/sender/v1/cast_sender.js?loadCastFramework=1";
  document.head.appendChild(s);
}

/** Opens Chrome's device picker. */
export function requestCast() {
  return window.cast?.framework.CastContext.getInstance().requestSession();
}

export function stopCast() {
  window.cast?.framework.CastContext.getInstance().getCurrentSession()?.endSession(true);
}

/** Sends a video to the connected receiver, which fetches it straight from the server. */
export async function castLoad(m: CastMedia) {
  const { cast, chrome } = window;
  const session = cast.framework.CastContext.getInstance().getCurrentSession();
  if (!session) return;
  const info = new chrome.cast.media.MediaInfo(m.url, m.contentType);
  info.streamType = chrome.cast.media.StreamType.BUFFERED;
  info.metadata = new chrome.cast.media.GenericMediaMetadata();
  info.metadata.title = m.title;
  info.metadata.subtitle = m.subtitle;
  if (m.image) info.metadata.images = [new chrome.cast.Image(m.image)];
  const req = new chrome.cast.media.LoadRequest(info);
  req.currentTime = m.start;
  if (m.vtt) {
    const t = new chrome.cast.media.Track(1, chrome.cast.media.TrackType.TEXT);
    t.trackContentId = m.vtt;
    t.trackContentType = "text/vtt";
    t.subtype = chrome.cast.media.TextTrackType.SUBTITLES;
    t.name = "Subtitles";
    info.tracks = [t];
    req.activeTrackIds = [1];
  }
  await session.loadMedia(req);
}

export function castToggle() {
  controller?.playOrPause();
}

export function castSeek(t: number) {
  if (!player) return;
  player.currentTime = t;
  controller.seek();
}

/** AirPlay picker of WebKit (Safari, iOS); null elsewhere. */
export function airplay(video: HTMLVideoElement): (() => void) | null {
  const v = video as HTMLVideoElement & { webkitShowPlaybackTargetPicker?: () => void };
  return window.WebKitPlaybackTargetAvailabilityEvent && v.webkitShowPlaybackTargetPicker ? () => v.webkitShowPlaybackTargetPicker!() : null;
}
