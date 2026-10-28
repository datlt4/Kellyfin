import { useNavigate, useParams, useSearchParams } from "@solidjs/router";
import { createEffect, createResource, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import Icon from "../components/Icon";
import { nativeFullscreen, setFullscreen, setPlaying as keepPlaying } from "../native";
import { airplay, castAvailable, castDevice, castLoad, castSeek, castToggle, initCast, remotePaused, remoteTime, requestCast, stopCast } from "../cast";
import { api, authToken, fmtClock, img, mediaUrl, server, streamLabel, subLabel, type ItemDetail } from "../api";
import {
  audioStreams, autoQuality, bandwidth, canCopyAudio, screenP, cueAt, defaultAudio, defaultSub, defaultSub2, mseHls, nativeHls, originalMode, parseVtt,
  preferredQuality, qualities, remember, subLang, type Cue, type Mode, type Quality,
} from "../playback";

type HlsT = import("hls.js").default;

const HIDE_MS = 3000;
const SPEEDS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];
const speedLabel = (r: number) => (r === 1 ? "Normal" : `${r}×`);
const REPORT_MS = 10000;

export default function Player() {
  const params = useParams();
  // Keyed so moving to the next episode builds a fresh player.
  return <Show when={params.id} keyed>{(id) => <PlayerView id={id} />}</Show>;
}

function PlayerView(props: { id: string }) {
  const [sp] = useSearchParams();
  const nav = useNavigate();
  const [item] = createResource(() => api<ItemDetail>(`/items/${props.id}`));
  let video!: HTMLVideoElement;
  let root!: HTMLDivElement;
  let controls!: HTMLDivElement;

  const [mode, setMode] = createSignal<Mode>("direct");
  const [offset, setOffset] = createSignal(0); // media time at video.currentTime == 0 (remux starts mid-file)
  const [time, setTime] = createSignal(0);
  const [scrub, setScrub] = createSignal<number | null>(null);
  const [playing, setPlaying] = createSignal(false);
  const [waiting, setWaiting] = createSignal(true);
  const [audio, setAudio] = createSignal<number | null>(null);
  const [sub, setSub] = createSignal("off");
  const [cues, setCues] = createSignal<Cue[]>([]);
  const [cue, setCue] = createSignal("");
  const [sub2, setSub2] = createSignal("off");
  const [cues2, setCues2] = createSignal<Cue[]>([]);
  const [cue2, setCue2] = createSignal("");
  const [menu, setMenu] = createSignal<"subs" | "subs2" | "audio" | "quality" | "speed" | null>(null);
  const [rate, setRateSignal] = createSignal(Number(localStorage.getItem("rate")) || 1);
  // defaultPlaybackRate survives the src swaps of remux restarts and quality changes.
  function setRate(r: number) {
    setRateSignal(r);
    video.defaultPlaybackRate = r;
    video.playbackRate = r;
    try {
      localStorage.setItem("rate", String(r));
    } catch {}
  }
  const stepRate = (dir: number) => {
    const i = SPEEDS.indexOf(rate());
    setRate(SPEEDS[Math.min(SPEEDS.length - 1, Math.max(0, (i < 0 ? SPEEDS.indexOf(1) : i) + dir))]);
  };
  const [volume, setVolume] = createSignal(1);
  const [muted, setMuted] = createSignal(false);
  const [fav, setFav] = createSignal(false);
  const [ui, setUi] = createSignal(true);
  const [error, setError] = createSignal("");
  const [upNext, setUpNext] = createSignal<number | null>(null);
  // Next episode starts by itself at the end unless turned off in the settings menu (remembered).
  const [autoNext, setAutoNext] = createSignal(localStorage.getItem("autoNext") !== "0");
  const toggleAutoNext = () => {
    setAutoNext(!autoNext());
    try {
      localStorage.setItem("autoNext", autoNext() ? "1" : "0");
    } catch {}
  };
  const [quality, setQuality] = createSignal<Quality>("orig");
  const [levelP, setLevelP] = createSignal<number | null>(null); // rendition hls.js is playing
  let hls: HlsT | null = null;

  const duration = () => item()?.duration ?? 0;
  // While a new stream is loading, the position is where we are going, not the old stream's.
  let pendingAt: number | null = null;
  const now = () => pendingAt ?? offset() + (video?.currentTime ?? 0);
  let pendingSkip = 0;
  let loadGen = 0;

  async function load(at: number) {
    const it = item()!;
    const gen = ++loadGen;
    pendingAt = at;
    setError("");
    setWaiting(true);
    hls?.destroy();
    hls = null;
    setLevelP(null);
    if (mode() === "hls") return loadHls(at, gen);
    const q = quality();
    if (typeof q === "string" && q.startsWith("ai:")) {
      // An AI-upscaled version: a plain MP4 played as-is.
      setOffset(0);
      pendingSkip = at;
      video.src = mediaUrl(`/items/${it.id}/upscaled/${encodeURIComponent(q.slice(3))}`);
    } else if (mode() === "direct") {
      setOffset(0);
      pendingSkip = at;
      video.src = mediaUrl(`/items/${it.id}/file`);
    } else {
      const ks = await api<number>(`/items/${it.id}/keyframe?at=${at.toFixed(2)}`).catch(() => at);
      if (gen !== loadGen) return;
      setOffset(ks);
      pendingSkip = at - ks;
      const q: Record<string, string | number> = { start: ks.toFixed(3) };
      if (audio() !== null) q.audio = audio()!;
      if (canCopyAudio(it, audio())) q.copy_audio = "true";
      video.src = mediaUrl(`/items/${it.id}/remux`, q);
    }
    video.play().catch(() => setPlaying(false));
  }

  /** Server transcode; hls.js on MSE browsers, the native player on iOS. */
  async function loadHls(at: number, gen: number) {
    const it = item()!;
    setOffset(0);
    const master = mediaUrl(`/items/${it.id}/hls/master.m3u8`, { audio: audio() ?? -1 });
    const q = quality();
    if (!mseHls()) {
      if (!nativeHls()) return fail("This device cannot play transcoded video.");
      pendingSkip = at;
      // Native HLS has no level API: a fixed quality loads that rendition's playlist.
      video.src = typeof q === "number" ? mediaUrl(`/items/${it.id}/hls/${q}/${audio() ?? -1}/index.m3u8`) : master;
      video.play().catch(() => setPlaying(false));
      return;
    }
    // hls.js ABR misreads transcoder start-up time as a slow network and thrashes between rungs,
    // and every rung change restarts the encoder. So levels are always fixed; "auto" picks one up front.
    const [{ default: Hls }, p] = await Promise.all([
      import("hls.js/light"),
      typeof q === "number" ? q : bandwidth(server(), authToken() ?? "").then((bps) => autoQuality(qualities(it), bps, screenP(root))),
    ]);
    if (gen !== loadGen) return;
    const rungs = qualities(it);
    // hls.js orders levels by ascending bitrate.
    const h = new Hls({ startPosition: at, maxBufferLength: 30, backBufferLength: 60, startLevel: rungs.length - 1 - rungs.indexOf(p) });
    hls = h;
    h.on(Hls.Events.MANIFEST_PARSED, () => {
      h.currentLevel = levelOf(p);
      video.play().catch(() => setPlaying(false));
    });
    h.on(Hls.Events.LEVEL_SWITCHED, (_e, d) => {
      const l = h.levels[d.level];
      setLevelP(l ? Math.min(l.width, l.height) : null);
    });
    h.on(Hls.Events.ERROR, (_e, d) => {
      if (!d.fatal) return;
      if (d.type === Hls.ErrorTypes.NETWORK_ERROR) h.startLoad();
      else if (d.type === Hls.ErrorTypes.MEDIA_ERROR) h.recoverMediaError();
      else fail("Transcoding failed.");
    });
    h.loadSource(master);
    h.attachMedia(video);
  }

  const levelOf = (p: number) => hls?.levels.findIndex((l) => Math.min(l.width, l.height) === p) ?? -1;

  // Auto steps down one rung after two stalls within a minute of steady playback.
  let stalls: number[] = [];
  function onStall() {
    if (!hls || quality() !== "auto" || pendingAt !== null || !playing()) return;
    const t = Date.now();
    stalls = [...stalls.filter((s) => t - s < 60_000), t];
    const rungs = qualities(item()!);
    const i = rungs.indexOf(levelP() ?? rungs[rungs.length - 1]);
    if (stalls.length >= 2 && i >= 0 && i < rungs.length - 1) {
      stalls = [];
      hls.currentLevel = levelOf(rungs[i + 1]);
    }
  }

  function fail(msg: string) {
    pendingAt = null;
    setWaiting(false);
    setError(msg);
  }

  /** Original file when the browser can play it, else a transcode. */
  function modeFor(it: ItemDetail, a: number | null, q: Quality): Mode {
    if (typeof q === "string" && q.startsWith("ai:")) return "direct";
    const m = (q === "orig" && originalMode(it, a)) || "hls";
    if (m === "hls" && q === "orig") setQuality("auto"); // original not playable here
    return m;
  }

  function changeQuality(v: string) {
    const q: Quality = v === "orig" || v === "auto" || v.startsWith("ai:") ? (v as Quality) : Number(v);
    const it = item()!;
    setQuality(q);
    remember("quality", String(q));
    const m = modeFor(it, audio(), q);
    if (m === "hls" && hls && typeof q === "number") {
      // Same stream, different rendition: hls.js switches without reloading.
      hls.currentLevel = levelOf(q);
      return;
    }
    setMode(m);
    load(now());
  }

  function seek(t: number) {
    t = Math.max(0, Math.min(t, Math.max(0, duration() - 1)));
    setTime(t);
    if (casting()) return castSeek(t);
    if (mode() !== "remux") {
      video.currentTime = t;
      return;
    }
    // Remux streams forward only: jump inside what is buffered, else restart from a keyframe.
    const rel = t - offset();
    const b = video.buffered;
    if (rel >= 0 && b.length && rel <= b.end(b.length - 1)) video.currentTime = rel;
    else load(t);
  }

  function report(keepalive = false) {
    const it = item();
    if (!it || !authToken()) return;
    const position = video.ended ? duration() : now();
    if (position < 1) return;
    const how = mode() === "direct" ? "Direct play" : mode() === "remux" ? "Remux" : "Transcode";
    api(`/items/${it.id}/progress`, { method: "POST", body: JSON.stringify({ position, mode: how }), keepalive }).catch(() => {});
  }

  // Initial choices once the item is known.
  createEffect(on(item, (it) => {
    if (!it) return;
    const a = sp.audio !== undefined ? Number(sp.audio) : defaultAudio(it);
    setAudio(a);
    setSub(typeof sp.sub === "string" ? sp.sub : defaultSub(it));
    setSub2(defaultSub2(it, sub()));
    setFav(it.favorite);
    const q = preferredQuality();
    // A remembered height the file does not have falls back to adaptive.
    setQuality(typeof q === "number" && !qualities(it).includes(q) ? "auto" : q);
    setMode(modeFor(it, a, quality()));
    // A watched video being watched again resumes too (finishing it resets the position to 0).
    const start = sp.t !== undefined ? Number(sp.t) : it.position;
    load(start > 5 ? start : 0);
    if ("mediaSession" in navigator) {
      const art = img(it, "m");
      navigator.mediaSession.metadata = new MediaMetadata({
        title: it.title,
        album: it.library_name,
        artwork: art ? [{ src: art, sizes: "800x450", type: "image/webp" }] : [],
      });
    }
  }));

  // Subtitles are drawn by us (not <track>) so they stay in sync across remux restarts.
  async function fetchCues(key: string): Promise<Cue[]> {
    const it = item();
    if (!it || key === "off") return [];
    try {
      const r = await fetch(`${server()}/api/items/${it.id}/subtitles/${key}.vtt`, {
        headers: { Authorization: `Bearer ${authToken()}` },
      });
      return r.ok ? parseVtt(await r.text()) : [];
    } catch {
      return [];
    }
  }
  createEffect(on(sub, async (key) => {
    setCues([]);
    setCue("");
    const c = await fetchCues(key);
    if (sub() === key) setCues(c);
  }));
  createEffect(on(sub2, async (key) => {
    setCues2([]);
    setCue2("");
    const c = await fetchCues(key);
    if (sub2() === key) setCues2(c);
  }));

  function changeAudio(index: number) {
    const it = item()!;
    const at = now();
    setAudio(index);
    remember("audioLang", audioStreams(it).find((s) => s.index === index)?.lang ?? "");
    setMode(modeFor(it, index, quality()));
    load(at);
  }

  function changeSub(key: string) {
    setSub(key);
    if (key === sub2()) setSub2("off");
    remember("subLang", subLang(item()?.subtitles.find((s) => s.key === key)));
  }

  function changeSub2(key: string) {
    setSub2(key);
    remember("sub2Lang", subLang(item()?.subtitles.find((s) => s.key === key)));
  }

  async function toggleFav() {
    const it = item();
    if (!it) return;
    setFav(!fav());
    await api(`/items/${it.id}/userdata`, { method: "POST", body: JSON.stringify({ favorite: fav() }) }).catch(() => setFav(!fav()));
  }

  function setVol(v: number) {
    video.volume = v;
    video.muted = v === 0;
  }
  // Drawn by hand: native range inputs differ per engine (WebKitGTK's thumb never reached the end).
  function volDrag(e: PointerEvent) {
    const bar = e.currentTarget as HTMLElement;
    const at = (x: number) => {
      const r = bar.getBoundingClientRect();
      setVol(Math.round(Math.min(1, Math.max(0, (x - r.left) / r.width)) * 20) / 20);
    };
    bar.setPointerCapture(e.pointerId);
    at(e.clientX);
    bar.onpointermove = (m) => at(m.clientX);
    bar.onpointerup = bar.onpointercancel = () => (bar.onpointermove = null);
  }

  // UI auto-hide while playing.
  let hideTimer: number | undefined;
  function poke() {
    setUi(true);
    clearTimeout(hideTimer);
    hideTimer = window.setTimeout(hide, HIDE_MS);
  }
  // Not while the mouse rests on the controls.
  const hide = () => playing() && !menu() && !root.querySelector(".p-top:hover, .p-bottom:hover") && setUi(false);
  /** Mouse left the player: fade the controls out after a second. */
  function onLeave(e: PointerEvent) {
    if (e.pointerType !== "mouse") return;
    clearTimeout(hideTimer);
    hideTimer = window.setTimeout(() => playing() && !menu() && setUi(false), 1000);
  }

  // Hand-drawn: an invisible range input mapped clicks with its thumb inset, so the fill landed short of the click.
  function seekDrag(e: PointerEvent) {
    if (!duration()) return;
    const bar = e.currentTarget as HTMLElement;
    const at = (x: number) => {
      const r = bar.getBoundingClientRect();
      return Math.min(1, Math.max(0, (x - r.left) / r.width)) * duration();
    };
    const done = () => (bar.onpointermove = bar.onpointerup = bar.onpointercancel = null);
    bar.setPointerCapture(e.pointerId);
    setScrub(at(e.clientX));
    bar.onpointermove = (m) => setScrub(at(m.clientX));
    bar.onpointerup = (u) => {
      done();
      seek(at(u.clientX));
      setScrub(null);
    };
    bar.onpointercancel = () => {
      done();
      setScrub(null);
    };
  }

  // While casting, the controls drive the TV instead of the local <video>.
  const casting = () => castDevice() !== null;
  const isPlaying = () => (casting() ? !remotePaused() : playing());
  const toggle = () => (casting() ? castToggle() : video.paused ? video.play().catch(() => {}) : video.pause());

  /** What the TV should fetch: the original MP4 when every Chromecast decodes it, else an H.264 transcode. */
  function castMedia() {
    const it = item()!;
    const original = it.video_codec === "h264" && ["mp4", "m4v"].includes(it.ext) && audio() === (audioStreams(it)[0]?.index ?? null);
    return {
      url: original ? mediaUrl(`/items/${it.id}/file`) : mediaUrl(`/items/${it.id}/hls/master.m3u8`, { audio: audio() ?? -1 }),
      contentType: original ? "video/mp4" : "application/x-mpegurl",
      title: it.title,
      subtitle: it.library_name,
      image: img(it, "l"),
      vtt: sub() !== "off" ? mediaUrl(`/items/${it.id}/subtitles/${sub()}.vtt`) : undefined,
      start: now(),
    };
  }
  createEffect(on(castDevice, (dev, prev) => {
    if (dev && item()) {
      video.pause();
      castLoad(castMedia()).catch((e) => setError(`Cast failed: ${e?.description ?? e}`));
    } else if (prev && !dev) {
      // Back from the TV: carry on here where it stopped.
      seek(remoteTime());
      video.play().catch(() => {});
    }
  }, { defer: true }));
  const [airplayPick, setAirplayPick] = createSignal<(() => void) | null>(null);

  const [nativeFs, setNativeFs] = createSignal(false);
  async function fullscreen() {
    if (nativeFullscreen) {
      // Android app: hide the system bars and turn landscape; Linux app: fullscreen window (see native.ts).
      setNativeFs(!nativeFs());
      setFullscreen(nativeFs());
      return;
    }
    if (document.fullscreenElement) return document.exitFullscreen();
    await root.requestFullscreen?.().catch(() => {});
    (screen.orientation as ScreenOrientation & { lock?: (o: string) => Promise<void> }).lock?.("landscape").catch(() => {});
  }

  const pipOk = "pictureInPictureEnabled" in document && document.pictureInPictureEnabled;
  const pip = () =>
    document.pictureInPictureElement ? document.exitPictureInPicture() : video.requestPictureInPicture().catch(() => {});

  // Black bars found by the scan: zoom the picture past them, unless turned off (remembered).
  const [cropOn, setCropOn] = createSignal(localStorage.getItem("crop") !== "0");
  const cropBox = () => {
    const it = item();
    const c = it?.crop?.split(":").map(Number);
    if (!it?.width || !it.height || c?.length !== 4) return null;
    return { w: c[0] / it.width, h: c[1] / it.height, x: c[2] / it.width, y: c[3] / it.height };
  };
  const [frame, setFrame] = createSignal({ bw: 0, bh: 0, vw: 0, vh: 0 });
  const measure = () => setFrame({ bw: root.clientWidth, bh: root.clientHeight, vw: video.videoWidth, vh: video.videoHeight });
  const cropTransform = () => {
    const c = cropBox(), f = frame();
    if (!cropOn() || !c || !f.vw || !f.bw) return undefined;
    const fit = Math.min(f.bw / f.vw, f.bh / f.vh); // object-fit: contain
    const zoom = Math.min(f.bw / (c.w * f.vw), f.bh / (c.h * f.vh));
    const tx = -(c.x + c.w / 2 - 0.5) * f.vw * zoom;
    const ty = -(c.y + c.h / 2 - 0.5) * f.vh * zoom;
    return `translate(${tx}px, ${ty}px) scale(${zoom / fit})`;
  };
  const toggleCrop = () => {
    setCropOn(!cropOn());
    try {
      localStorage.setItem("crop", cropOn() ? "1" : "0");
    } catch {}
  };

  const [hud, setHud] = createSignal<{ kind: "speed"; v: number; note?: string } | null>(null);
  let hudTimer: number | undefined;

  // Touch: single tap shows/hides controls, double tap on a side skips 10s. Mouse: click toggles play.
  let lastTap = 0;
  function onTap(e: PointerEvent) {
    if (e.pointerType === "mouse") return toggle();
    const t = Date.now();
    const x = e.clientX / root.clientWidth;
    if (t - lastTap < 300 && (x < 0.35 || x > 0.65)) {
      seek(now() + (x < 0.5 ? -10 : 10));
      lastTap = 0;
      return;
    }
    lastTap = t;
    ui() ? setUi(false) : poke();
  }

  function onKey(e: KeyboardEvent) {
    if (e.target instanceof HTMLSelectElement || e.target instanceof HTMLInputElement) return;
    const k = e.key;
    // Alt+P / Option+P (on a Mac the key reads "π", so match the physical key).
    if (e.altKey && e.code === "KeyP") pip();
    else if (k === " " || k === "k") toggle();
    else if (k === "ArrowLeft" || k === "j") seek(now() - 10);
    else if (k === "ArrowRight" || k === "l") seek(now() + 10);
    else if (k === "f") fullscreen();
    else if (k === "m") video.muted = !video.muted;
    else if (k === "<" || k === ">") {
      stepRate(k === ">" ? 1 : -1);
      setHud({ kind: "speed", v: (SPEEDS.indexOf(rate()) + 1) / SPEEDS.length, note: speedLabel(rate()) });
      clearTimeout(hudTimer);
      hudTimer = window.setTimeout(() => setHud(null), 900);
    }
    else if (k === "Escape" && nativeFs()) fullscreen();
    else if (k === "Escape" && !document.fullscreenElement) history.back();
    else return;
    e.preventDefault();
    poke();
  }

  // Tab close / app backgrounding never runs cleanup, so save then too.
  const onHide = () => document.visibilityState === "hidden" && report(true);
  const onPageHide = () => report(true);

  onMount(() => {
    // Subtitles sit just above the controls, whose height depends on how many rows they wrap to.
    initCast();
    setAirplayPick(() => airplay(video));
    const ro = new ResizeObserver(() => root.style.setProperty("--controls-h", `${controls.offsetHeight}px`));
    ro.observe(controls);
    const rr = new ResizeObserver(measure);
    rr.observe(root);
    onCleanup(() => { ro.disconnect(); rr.disconnect(); });
    window.addEventListener("keydown", onKey);
    window.addEventListener("pagehide", onPageHide);
    document.addEventListener("visibilitychange", onHide);
    let raf = 0;
    const tick = () => {
      const t = casting() ? remoteTime() : now();
      if (scrub() === null) setTime(t);
      const text = cueAt(cues(), t);
      if (text !== cue()) setCue(text);
      const text2 = cueAt(cues2(), t);
      if (text2 !== cue2()) setCue2(text2);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    const timer = setInterval(() => playing() && report(), REPORT_MS);
    onCleanup(() => {
      report(true);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pagehide", onPageHide);
      document.removeEventListener("visibilitychange", onHide);
      cancelAnimationFrame(raf);
      clearInterval(timer);
      clearTimeout(hideTimer);
      clearTimeout(hudTimer);
      clearInterval(nextTimer);
      hls?.destroy();
      if (nativeFs()) setFullscreen(false);
      keepPlaying(false);
      video.removeAttribute("src");
      video.load(); // stops the download / server-side remux
    });
  });

  let nextTimer: number | undefined;
  function onEnded() {
    if (upNext() !== null) return; // both the end-of-stream stall and "ended" fired
    report();
    const next = item()?.next_id;
    if (!next || !autoNext()) return;
    setUpNext(5);
    nextTimer = window.setInterval(() => {
      const n = upNext()! - 1;
      if (n <= 0) {
        clearInterval(nextTimer);
        nav(`/play/${next}`, { replace: true });
      } else setUpNext(n);
    }, 1000);
  }

  const shown = () => scrub() ?? time();
  const pct = () => (duration() ? (shown() / duration()) * 100 : 0);
  const bufferedPct = () => {
    time(); // re-evaluate as playback advances
    const b = video?.buffered;
    if (!b?.length || !duration()) return 0;
    return ((offset() + b.end(b.length - 1)) / duration()) * 100;
  };

  return (
    <div class="player" classList={{ "ui-hidden": !ui() }} ref={root} onPointerMove={(e) => e.pointerType === "mouse" && poke()} onPointerLeave={onLeave}>
      <video
        ref={video}
        x-webkit-airplay="allow"
        playsinline
        preload="auto"
        poster={item() ? img(item()!, "l") : undefined}
        onPlay={() => { setPlaying(true); keepPlaying(true); poke(); }}
        onPause={() => { setPlaying(false); keepPlaying(false); setUi(true); report(); }}
        onWaiting={() => {
          // The playlist's last segment can promise a few ms more than the encode holds; iOS then waits forever.
          if (duration() - now() < 2) {
            video.pause();
            setWaiting(false);
            return onEnded();
          }
          setWaiting(true);
          onStall();
        }}
        onPlaying={() => { setWaiting(false); pendingAt = null; }}
        onCanPlay={() => setWaiting(false)}
        onSeeked={() => report()}
        onVolumeChange={() => { setVolume(video.volume); setMuted(video.muted); }}
        onLoadedData={() => (video.playbackRate = rate())}
        onLoadedMetadata={() => {
          measure();
          if (pendingSkip > 0.3) video.currentTime = pendingSkip;
          pendingSkip = 0;
          pendingAt = null;
        }}
        onEnded={onEnded}
        onError={() => {
          if (!video.getAttribute("src") || hls) return; // hls.js reports its own errors
          const at = now();
          pendingAt = null;
          // Browser refused the file after all: fall back to a remux, then to a transcode.
          if (mode() === "direct") setMode("remux");
          else if (mode() === "remux") setMode("hls");
          else return fail("Playback failed.");
          load(at);
        }}
        onPointerUp={onTap}
        onResize={measure}
        style={{ transform: cropTransform() }}
      />

      <Show when={hud()}>
        {(h) => (
          <div class="hud">
            <Icon name={h().kind} size={26} />
            <div class="hud-bar"><div style={{ width: `${Math.round(h().v * 100)}%` }} /></div>
            <Show when={h().note}><small>{h().note}</small></Show>
          </div>
        )}
      </Show>

      <Show when={cue() || cue2()}>
        <div class="subs">
          <Show when={cue2()}><span class="sub2">{cue2()}</span></Show>
          <Show when={cue()}><span>{cue()}</span></Show>
        </div>
      </Show>

      <Show when={menu()}>
        {(m) => {
          const it = item()!;
          const pick = (f: (v: string) => void) => (v: string) => { f(v); setMenu(null); };
          const opts = (): { title: string; current: string; items: { value: string; label: string; off?: boolean }[]; onPick: (v: string) => void } => {
            const subs = it.subtitles.map((s) => ({ value: s.key, label: subLabel(s), off: !s.supported }));
            switch (m()) {
              case "subs":
                return { title: "Subtitles", current: sub(), items: [{ value: "off", label: "Off" }, ...subs], onPick: pick(changeSub) };
              case "subs2":
                return {
                  title: "Secondary subtitles",
                  current: sub2(),
                  items: [{ value: "off", label: "Off" }, ...subs.filter((s) => s.value !== sub())],
                  onPick: pick(changeSub2),
                };
              case "audio":
                return {
                  title: "Audio",
                  current: String(audio()),
                  items: audioStreams(it).map((s) => ({ value: String(s.index), label: streamLabel(s) })),
                  onPick: pick((v) => changeAudio(Number(v))),
                };
              case "speed":
                return {
                  title: "Playback speed",
                  current: String(rate()),
                  items: SPEEDS.map((r) => ({ value: String(r), label: speedLabel(r) })),
                  onPick: pick((v) => setRate(Number(v))),
                };
              default:
                return {
                  title: "Quality",
                  current: String(quality()),
                  items: [
                    ...(originalMode(it, audio()) ? [{ value: "orig", label: "Original" }] : []),
                    { value: "auto", label: `Auto${mode() === "hls" && quality() === "auto" && levelP() ? ` (${levelP()}p)` : ""}` },
                    ...qualities(it).map((p) => ({ value: String(p), label: `${p}p` })),
                    ...it.upscaled.map((u) => ({ value: `ai:${u.model}`, label: `AI · ${u.model}${u.height ? ` (${Math.min(u.width ?? u.height, u.height)}p)` : ""}` })),
                  ],
                  onPick: pick(changeQuality),
                };
            }
          };
          return (
            <>
              <div class="menu-shade" onPointerUp={(e) => { e.stopPropagation(); setMenu(null); }} />
              <div class="p-menu" onPointerUp={(e) => e.stopPropagation()}>
                <div class="p-menu-title">
                  <Show when={m() === "subs2" || m() === "speed"}>
                    <button class="icon small" onClick={() => setMenu(m() === "speed" ? "quality" : "subs")} aria-label="Back"><Icon name="left" size={18} /></button>
                  </Show>
                  {opts().title}
                </div>
                <Show when={m() === "quality"}>
                  <button class="p-menu-item" onClick={toggleAutoNext}>
                    <span class="check">{autoNext() ? <Icon name="check" size={18} /> : null}</span>
                    <span class="grow">Auto-play next</span>
                    <span class="muted">{autoNext() ? "On" : "Off"}</span>
                  </button>
                  <button class="p-menu-item" onClick={() => setMenu("speed")}>
                    <span class="check" />
                    <span class="grow">Playback speed</span>
                    <span class="muted">{speedLabel(rate())}</span>
                    <Icon name="right" size={18} />
                  </button>
                </Show>
                <Show when={m() === "subs" && it.subtitles.length > 1}>
                  <button class="p-menu-item" onClick={() => setMenu("subs2")}>
                    <span class="check" />
                    <span class="grow">Secondary subtitles</span>
                    <span class="muted">{sub2() === "off" ? "Off" : subLabel(it.subtitles.find((s) => s.key === sub2())!)}</span>
                    <Icon name="right" size={18} />
                  </button>
                </Show>
                <For each={opts().items}>
                  {(o) => (
                    <button class="p-menu-item" disabled={o.off} onClick={() => opts().onPick(o.value)}>
                      <span class="check">{o.value === opts().current ? <Icon name="check" size={18} /> : null}</span>
                      <span class="grow">{o.label}</span>
                    </button>
                  )}
                </For>
              </div>
            </>
          );
        }}
      </Show>

      <Show when={waiting() && !error()}><div class="spinner" /></Show>
      <Show when={error()}><div class="player-msg">{error()}</div></Show>
      <Show when={castDevice()}>
        {(d) => (
          <div class="cast-banner">
            <Icon name="cast" size={40} />
            <span>Playing on <b>{d()}</b></span>
          </div>
        )}
      </Show>
      <Show when={upNext() !== null}>
        <button class="up-next" onClick={() => nav(`/play/${item()!.next_id}`, { replace: true })}>
          Next episode in {upNext()}s ›
        </button>
      </Show>

      <div class="p-top">
        <button class="icon" onClick={() => history.back()} aria-label="Back"><Icon name="back" /></button>
        <div class="p-title">
          <span class="muted">{item()?.library_name}</span>
          <strong>{item()?.title}</strong>
        </div>
        <span class="grow" />
        <Show when={castAvailable()}>
          <button class="icon" classList={{ on: casting() }} onClick={() => (casting() ? stopCast() : requestCast())} aria-label="Cast">
            <Icon name="cast" />
          </button>
        </Show>
        <Show when={airplayPick()}>{(pick) => <button class="icon" onClick={() => pick()()} aria-label="AirPlay"><Icon name="airplay" /></button>}</Show>
        <Show when={cropBox()}>
          <button class="icon" classList={{ on: cropOn() }} onClick={toggleCrop} aria-label="Crop black bars" title="Crop black bars"><Icon name="crop" /></button>
        </Show>
        <Show when={pipOk}><button class="icon" onClick={pip} aria-label="Picture in picture"><Icon name="pip" /></button></Show>
      </div>

      {/* Play/pause and ±10 s in the middle, the YouTube way. */}
      <div class="p-center" onPointerUp={(e) => e.stopPropagation()}>
        <button class="icon big" onClick={() => seek(now() - 10)} aria-label="Back 10 seconds"><Icon name="back10" size={34} /></button>
        <button class="icon huge" onClick={toggle} aria-label={isPlaying() ? "Pause" : "Play"}><Icon name={isPlaying() ? "pause" : "play"} size={44} /></button>
        <button class="icon big" onClick={() => seek(now() + 10)} aria-label="Forward 10 seconds"><Icon name="fwd10" size={34} /></button>
      </div>

      <div class="p-bottom" ref={controls} onPointerUp={(e) => e.stopPropagation()}>
        <div class="seek" classList={{ dragging: scrub() !== null }} onPointerDown={seekDrag} role="slider" aria-label="Seek" aria-valuemin={0} aria-valuemax={duration()} aria-valuenow={Math.round(shown())}>
          <div class="seek-buf" style={{ width: `${bufferedPct()}%` }} />
          <div class="seek-fill" style={{ width: `${pct()}%` }} />
          <div class="seek-thumb" style={{ left: `${pct()}%` }} />
        </div>
        <div class="p-row">
          <Show when={item()?.prev_id}>
            <button class="icon" onClick={() => nav(`/play/${item()!.prev_id}`, { replace: true })} aria-label="Previous episode"><Icon name="prev" /></button>
          </Show>
          <span class="clock">{fmtClock(shown())} / {fmtClock(duration())}</span>
          <Show when={item()?.next_id}>
            <button class="icon" onClick={() => nav(`/play/${item()!.next_id}`, { replace: true })} aria-label="Next episode"><Icon name="next" /></button>
          </Show>
          <span class="grow" />
          <button class="icon" classList={{ on: fav() }} onClick={toggleFav} aria-label="Favorite"><Icon name="heart" /></button>
          <Show when={item()?.subtitles.length}>
            <button class="icon" classList={{ on: sub() !== "off" }} onClick={() => setMenu(menu() === "subs" ? null : "subs")} aria-label="Subtitles"><Icon name="cc" /></button>
          </Show>
          <Show when={item() && audioStreams(item()!).length > 1}>
            <button class="icon" onClick={() => setMenu(menu() === "audio" ? null : "audio")} aria-label="Audio track"><Icon name="music" /></button>
          </Show>
          <div class="vol">
            <button class="icon" onClick={() => (video.muted = !video.muted)} aria-label={muted() ? "Unmute" : "Mute"}>
              <Icon name={muted() || volume() === 0 ? "mute" : "volume"} />
            </button>
            <div class="vol-bar" style={{ "--v": muted() ? 0 : volume() }} onPointerDown={volDrag} role="slider" aria-label="Volume" aria-valuemin={0} aria-valuemax={1} aria-valuenow={muted() ? 0 : volume()}>
              <div class="vol-fill" />
              <div class="vol-thumb" />
            </div>
          </div>
          <Show when={item()}>
            <button class="icon" onClick={() => setMenu(menu() === "quality" ? null : "quality")} aria-label="Quality"><Icon name="gear" /></button>
          </Show>
          <button class="icon" onClick={fullscreen} aria-label="Fullscreen"><Icon name="fullscreen" /></button>
        </div>
        <span class="p-mode">
          {String(quality()).startsWith("ai:") ? `AI upscale · ${String(quality()).slice(3)}` : mode() === "direct" ? "Direct play" : mode() === "remux" ? "Remux" : `Transcode${levelP() ? ` ${levelP()}p` : ""}`}
        </span>
      </div>
    </div>
  );
}
