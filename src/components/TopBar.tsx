import { A, useNavigate } from "@solidjs/router";
import { createResource, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { activeServer, api, appVersion, img, logout, savedServers, serverVersion, switchServer, switchTo, user, type Lib } from "../api";
import Icon from "./Icon";

const fold = (s: string) => s.normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/đ/gi, "d").toLowerCase();
const narrow = () => window.matchMedia("(max-width: 640px)").matches;

/** Library list as plain text: reach any library without scrolling through thumbnails. */
function Sidebar(props: { libs: Lib[]; onClose: () => void }) {
  const [filter, setFilter] = createSignal("");
  const shown = () => props.libs.filter((l) => fold(l.name).includes(fold(filter().trim())));
  const onKey = (e: KeyboardEvent) => e.key === "Escape" && props.onClose();
  onMount(() => window.addEventListener("keydown", onKey));
  onCleanup(() => window.removeEventListener("keydown", onKey));
  return (
    <>
      <div class="drawer-shade" onClick={props.onClose} />
      <aside class="drawer" aria-label="Libraries">
        <div class="drawer-head">
          <A href="/" class="brand" end onClick={props.onClose}>
            <img src="/favicon.svg" alt="" width="26" height="26" />
            <span>Kellyfin</span>
          </A>
          <button class="icon" onClick={props.onClose} aria-label="Close"><Icon name="close" /></button>
        </div>
        <A href="/" class="drawer-item" end onClick={props.onClose}>Home</A>
        <A href="/search" class="drawer-item" onClick={props.onClose}>Search</A>
        <input class="drawer-filter" type="search" placeholder={`Filter ${props.libs.length} libraries`} value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} />
        <nav class="drawer-list">
          <For each={shown()} fallback={<p class="muted pad">No library matches.</p>}>
            {(l) => (
              <A href={`/library/${l.id}`} class="drawer-item" onClick={props.onClose}>
                <span>{l.name}</span>
                <small>{l.count}</small>
              </A>
            )}
          </For>
        </nav>
      </aside>
    </>
  );
}

export default function TopBar() {
  const [libs] = createResource(() => api<Lib[]>("/libraries"));
  const [menu, setMenu] = createSignal(false);
  const [drawer, setDrawer] = createSignal(false);
  const nav = useNavigate();
  // A mouse wheel only scrolls vertically: turn it into horizontal scrolling of the library chips.
  const wheel = (e: WheelEvent) => {
    const el = e.currentTarget as HTMLElement;
    if (Math.abs(e.deltaY) <= Math.abs(e.deltaX) || el.scrollWidth <= el.clientWidth) return;
    el.scrollLeft += e.deltaY;
    e.preventDefault();
  };
  return (
    <header class="topbar">
      <button class="icon menu-btn" onClick={() => setDrawer(true)} aria-label="Libraries"><Icon name="menu" /></button>
      <A
        href="/"
        class="brand"
        end
        onClick={(e) => {
          // Phones have no room for the menu button: the logo opens the library list.
          if (narrow()) {
            e.preventDefault();
            setDrawer(true);
          }
        }}
      >
        <img src="/favicon.svg" alt="" width="26" height="26" />
        <span>Kellyfin</span>
      </A>
      <nav class="chips" onWheel={wheel}>
        <For each={libs()}>{(l) => <A href={`/library/${l.id}`} class="chip">{l.name}</A>}</For>
      </nav>
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
      <Show when={drawer()}>
        <Sidebar libs={libs() ?? []} onClose={() => setDrawer(false)} />
      </Show>
    </header>
  );
}
