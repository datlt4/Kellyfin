import { A, useNavigate } from "@solidjs/router";
import { createResource, createSignal, For, Show } from "solid-js";
import { api, img, logout, user, type Lib } from "../api";

export default function TopBar() {
  const [libs] = createResource(() => api<Lib[]>("/libraries"));
  const [menu, setMenu] = createSignal(false);
  const nav = useNavigate();
  return (
    <header class="topbar">
      <A href="/" class="brand" end>
        <img src="/favicon.svg" alt="" width="26" height="26" />
        <span>Kellyfin</span>
      </A>
      <nav class="chips">
        <For each={libs()}>{(l) => <A href={`/library/${l.id}`} class="chip">{l.name}</A>}</For>
      </nav>
      <A href="/search" class="icon search-btn" aria-label="Search">⌕</A>
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
            <button onClick={async () => { await logout(); nav("/"); }}>Sign out</button>
          </div>
        </Show>
      </div>
    </header>
  );
}
