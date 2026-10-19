import { useNavigate, useParams, useSearchParams } from "@solidjs/router";
import { createEffect, createResource, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import { api, authToken, fmtClock, img, mediaUrl, server, streamLabel, subLabel, type ItemDetail } from "../api";
import { audioStreams, canCopyAudio, cueAt, defaultAudio, defaultSub, parseVtt, pickMode, remember, subLang, type Cue, type Mode } from "../playback";

const HIDE_MS = 3000;
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
  const [ui, setUi] = createSignal(true);
  const [error, setError] = createSignal("");
  const [upNext, setUpNext] = createSignal<number | null>(null);

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
    if (mode() === "unsupported") {
      setError(`${it.video_codec?.toUpperCase() ?? "This"} video needs transcoding, which arrives with P3.`);
      pendingAt = null;
      return;
    }
    if (mode() === "direct") {
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

  function seek(t: number) {
    t = Math.max(0, Math.min(t, Math.max(0, duration() - 1)));
    setTime(t);
    if (mode() === "direct") {
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
    api(`/items/${it.id}/progress`, { method: "POST", body: JSON.stringify({ position }), keepalive }).catch(() => {});
  }

  // Initial choices once the item is known.
  createEffect(on(item, (it) => {
    if (!it) return;
    const a = sp.audio !== undefined ? Number(sp.audio) : defaultAudio(it);
    setAudio(a);
    setSub(typeof sp.sub === "string" ? sp.sub : defaultSub(it));
    setMode(pickMode(it, a));
    const start = sp.t !== undefined ? Number(sp.t) : it.played ? 0 : it.position;
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
  createEffect(on(sub, async (key) => {
    setCues([]);
    setCue("");
    const it = item();
    if (!it || key === "off") return;
    try {
      const r = await fetch(`${server()}/api/items/${it.id}/subtitles/${key}.vtt`, {
        headers: { Authorization: `Bearer ${authToken()}` },
      });
      if (r.ok && sub() === key) setCues(parseVtt(await r.text()));
    } catch {}
  }));

  function changeAudio(index: number) {
    const it = item()!;
    const at = now();
    setAudio(index);
    remember("audioLang", audioStreams(it).find((s) => s.index === index)?.lang ?? "");
    setMode(pickMode(it, index));
    load(at);
  }

  function changeSub(key: string) {
    setSub(key);
    remember("subLang", subLang(item()?.subtitles.find((s) => s.key === key)));
  }

  // UI auto-hide while playing.
  let hideTimer: number | undefined;
  function poke() {
    setUi(true);
    clearTimeout(hideTimer);
    hideTimer = window.setTimeout(() => playing() && setUi(false), HIDE_MS);
  }

  const toggle = () => (video.paused ? video.play().catch(() => {}) : video.pause());

  async function fullscreen() {
    if (document.fullscreenElement) return document.exitFullscreen();
    await root.requestFullscreen?.().catch(() => {});
    (screen.orientation as ScreenOrientation & { lock?: (o: string) => Promise<void> }).lock?.("landscape").catch(() => {});
  }

  const pipOk = "pictureInPictureEnabled" in document && document.pictureInPictureEnabled;
  const pip = () =>
    document.pictureInPictureElement ? document.exitPictureInPicture() : video.requestPictureInPicture().catch(() => {});

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
    if (k === " " || k === "k") toggle();
    else if (k === "ArrowLeft" || k === "j") seek(now() - 10);
    else if (k === "ArrowRight" || k === "l") seek(now() + 10);
    else if (k === "f") fullscreen();
    else if (k === "m") video.muted = !video.muted;
    else if (k === "Escape" && !document.fullscreenElement) history.back();
    else return;
    e.preventDefault();
    poke();
  }

  // Tab close / app backgrounding never runs cleanup, so save then too.
  const onHide = () => document.visibilityState === "hidden" && report(true);
  const onPageHide = () => report(true);

  onMount(() => {
    window.addEventListener("keydown", onKey);
    window.addEventListener("pagehide", onPageHide);
    document.addEventListener("visibilitychange", onHide);
    let raf = 0;
    const tick = () => {
      const t = now();
      if (scrub() === null) setTime(t);
      const text = cueAt(cues(), t);
      if (text !== cue()) setCue(text);
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
      clearInterval(nextTimer);
      video.removeAttribute("src");
      video.load(); // stops the download / server-side remux
    });
  });

  let nextTimer: number | undefined;
  function onEnded() {
    report();
    const next = item()?.next_id;
    if (!next) return;
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
    <div class="player" classList={{ "ui-hidden": !ui() }} ref={root} onPointerMove={(e) => e.pointerType === "mouse" && poke()}>
      <video
        ref={video}
        playsinline
        preload="auto"
        poster={item() ? img(item()!, "l") : undefined}
        onPlay={() => { setPlaying(true); poke(); }}
        onPause={() => { setPlaying(false); setUi(true); report(); }}
        onWaiting={() => setWaiting(true)}
        onPlaying={() => setWaiting(false)}
        onCanPlay={() => setWaiting(false)}
        onSeeked={() => report()}
        onLoadedMetadata={() => {
          if (pendingSkip > 0.3) video.currentTime = pendingSkip;
          pendingSkip = 0;
          pendingAt = null;
        }}
        onEnded={onEnded}
        onError={() => {
          if (!video.getAttribute("src")) return;
          pendingAt = null;
          if (mode() === "direct" && item()) {
            // Browser refused the file after all: fall back to a server remux.
            setMode("remux");
            load(now());
          } else setError("Playback failed.");
        }}
        onPointerUp={onTap}
      />

      <Show when={cue()}>
        <div class="subs"><span>{cue()}</span></div>
      </Show>

      <Show when={waiting() && !error()}><div class="spinner" /></Show>
      <Show when={error()}><div class="player-msg">{error()}</div></Show>
      <Show when={upNext() !== null}>
        <button class="up-next" onClick={() => nav(`/play/${item()!.next_id}`, { replace: true })}>
          Next episode in {upNext()}s ›
        </button>
      </Show>

      <div class="p-top">
        <button class="icon" onClick={() => history.back()} aria-label="Back">←</button>
        <div class="p-title">
          <span class="muted">{item()?.library_name}</span>
          <strong>{item()?.title}</strong>
        </div>
      </div>

      <div class="p-bottom" onPointerUp={(e) => e.stopPropagation()}>
        <div class="seek">
          <div class="seek-buf" style={{ width: `${bufferedPct()}%` }} />
          <div class="seek-fill" style={{ width: `${pct()}%` }} />
          <input
            type="range"
            min="0"
            max={duration() || 0}
            step="1"
            value={shown()}
            onInput={(e) => setScrub(Number(e.currentTarget.value))}
            onChange={(e) => { seek(Number(e.currentTarget.value)); setScrub(null); }}
            aria-label="Seek"
          />
        </div>
        <div class="p-row">
          <button class="icon" onClick={toggle} aria-label={playing() ? "Pause" : "Play"}>{playing() ? "❚❚" : "▶"}</button>
          <button class="icon" onClick={() => seek(now() - 10)} aria-label="Back 10 seconds">↺</button>
          <button class="icon" onClick={() => seek(now() + 10)} aria-label="Forward 10 seconds">↻</button>
          <span class="clock">{fmtClock(shown())} / {fmtClock(duration())}</span>
          <span class="grow" />
          <Show when={item() && audioStreams(item()!).length > 1}>
            <select value={audio() ?? ""} onChange={(e) => changeAudio(Number(e.currentTarget.value))} aria-label="Audio">
              <For each={audioStreams(item()!)}>{(s) => <option value={s.index}>{streamLabel(s)}</option>}</For>
            </select>
          </Show>
          <Show when={item()?.subtitles.length}>
            <select value={sub()} onChange={(e) => changeSub(e.currentTarget.value)} aria-label="Subtitles">
              <option value="off">Subtitles off</option>
              <For each={item()!.subtitles}>
                {(s) => <option value={s.key} disabled={!s.supported}>{subLabel(s)}</option>}
              </For>
            </select>
          </Show>
          <Show when={item()?.next_id}>
            <button class="icon" onClick={() => nav(`/play/${item()!.next_id}`, { replace: true })} aria-label="Next episode">⏭</button>
          </Show>
          <Show when={pipOk}><button class="icon" onClick={pip} aria-label="Picture in picture">⧉</button></Show>
          <button class="icon" onClick={fullscreen} aria-label="Fullscreen">⛶</button>
        </div>
        <span class="p-mode">{mode() === "direct" ? "Direct play" : mode() === "remux" ? "Remux" : ""}</span>
      </div>
    </div>
  );
}
