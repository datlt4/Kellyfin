import { A, useLocation, useNavigate } from "@solidjs/router";
import { createEffect, createResource, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import { activeServer, api, appVersion, img, logout, savedServers, serverVersion, switchServer, switchTo, user, type Lib } from "../api";
import Icon from "./Icon";

const fold = (s: string) => s.normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/đ/gi, "d").toLowerCase();
const narrow = () => window.matchMedia("(max-width: 640px)").matches;
const GAP = 6; // .chips gap, px

/** Plain list of libraries with a filter (Jellyfin's "More" menu): any library without loading thumbnails. */
function LibraryMenu(props: { libs: Lib[]; onClose: () => void }) {
  const [filter, setFilter] = createSignal("");
  const shown = () => props.libs.filter((l) => fold(l.name).includes(fold(filter().trim())));
  const onKey = (e: KeyboardEvent) => e.key === "Escape" && props.onClose();
  onMount(() => window.addEventListener("keydown", onKey));
  onCleanup(() => window.removeEventListener("keydown", onKey));
  return (
    <>
      <div class="more-shade" onClick={props.onClose} />
      <div class="more-menu">
        <input class="more-filter" type="search" placeholder={`Filter ${props.libs.length} libraries`} value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} />
        <nav class="more-list">
          <For each={shown()} fallback={<p class="muted pad">No library matches.</p>}>
            {(l) => (
              <A href={`/library/${l.id}`} class="more-item" onClick={props.onClose}>
                <Icon name="film" size={18} />
                <span>{l.name}</span>
                <small>{l.count}</small>
              </A>
            )}
          </For>
        </nav>
      </div>
    </>
  );
}

export default function TopBar() {
  const [libs] = createResource(() => api<Lib[]>("/libraries"));
  const [menu, setMenu] = createSignal(false);
  const [more, setMore] = createSignal(false);
  // How many library chips fit in the bar; the rest go under "More".
  const [fit, setFit] = createSignal(Infinity);
  const nav = useNavigate();
  const loc = useLocation();
  let chips!: HTMLElement;

  // Sums chip widths (hidden chips keep theirs) so the result does not depend on the current layout;
  // the More button always holds its place, so the bar's width does not change with the result.
  const measure = () => {
    if (!chips) return;
    const limit = chips.clientWidth;
    let used = 0;
    const all = [...chips.querySelectorAll<HTMLElement>(".chip")];
    const n = all.findIndex((c, i) => (used += c.offsetWidth + (i ? GAP : 0)) > limit);
    setFit(n < 0 ? Infinity : n);
  };
  onMount(() => {
    const ro = new ResizeObserver(() => measure());
    ro.observe(chips);
    onCleanup(() => ro.disconnect());
  });
  createEffect(on(libs, () => requestAnimationFrame(measure)));

  const back = () => (history.length > 1 ? history.back() : nav("/"));

  return (
    <header class="topbar">
      <Show when={loc.pathname !== "/"}>
        <button class="icon back-btn" onClick={back} aria-label="Back"><Icon name="back" /></button>
      </Show>
      <A
        href="/"
        class="brand"
        end
        onClick={(e) => {
          // Phones have no room for the library bar: the logo opens the library list.
          if (narrow()) {
            e.preventDefault();
            setMore(true);
          }
        }}
      >
        <img src="/favicon.svg" alt="" width="26" height="26" />
        <span>Kellyfin</span>
      </A>
      <nav class="chips" ref={chips}>
        <For each={libs()}>
          {(l, i) => (
            <A href={`/library/${l.id}`} class="chip" classList={{ "chip-hidden": i() >= fit() }}>{l.name}</A>
          )}
        </For>
      </nav>
      <button class="more-btn" classList={{ unused: fit() >= (libs()?.length ?? 0) }} onClick={() => setMore(!more())}>More ▾</button>
      <A href="/search" class="icon search-btn" aria-label="Search"><Icon name="search" /></A>
      <div class="avatar-wrap">
        <button class="avatar" onClick={() => setMenu(!menu())} aria-label="Account">
          <Show when={user()?.img} fallback={user()?.name.slice(0, 1).toUpperCase()}>
            <img src={img({ img: user()!.img!, imgv: user()!.imgv ?? 0 }, "s")} alt="" />
          </Show>
        </button>
        <Show when={menu()}>
          <div class="menu" onClick={() => setMenu(false)}>
            <div class="menu-head">{user()?.name}</div>
            <A href="/settings/profile">Profile</A>
            <Show when={user()?.is_admin}>
              <A href="/settings/dashboard">Dashboard</A>
              <A href="/settings/libraries">Libraries</A>
            </Show>
            <div class="menu-head">Server</div>
            <For each={savedServers()}>
              {(s) => (
                <button classList={{ current: s.id === activeServer()?.id }} onClick={async () => { await switchTo(s); nav("/"); }}>
                  {s.id === activeServer()?.id ? "✓ " : ""}{s.name}{s.headers.length ? " 🔒" : ""}
                </button>
              )}
            </For>
            <button onClick={() => { switchServer(); nav("/"); }}>Add / edit servers…</button>
            <button onClick={async () => { await logout(); nav("/"); }}>Sign out</button>
            <div class="menu-foot">App {appVersion}{serverVersion() ? ` · Server ${serverVersion()}` : ""}</div>
          </div>
        </Show>
      </div>
      <Show when={more()}>
        {/* On wide screens only the libraries that did not fit; on phones all of them. */}
        <LibraryMenu libs={narrow() ? libs() ?? [] : (libs() ?? []).slice(fit())} onClose={() => setMore(false)} />
      </Show>
    </header>
  );
}
