import { A, useNavigate } from "@solidjs/router";
import { createResource, createSignal, For, Show } from "solid-js";
import { api, logout, user, type Lib } from "../api";

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
      <div class="avatar-wrap">
        <button class="avatar" onClick={() => setMenu(!menu())} aria-label="Account">
          {user()?.name.slice(0, 1).toUpperCase()}
        </button>
        <Show when={menu()}>
          <div class="menu" onClick={() => setMenu(false)}>
            <div class="menu-head">{user()?.name}</div>
            <A href="/settings">Settings</A>
            <button onClick={async () => { await logout(); nav("/"); }}>Sign out</button>
          </div>
        </Show>
      </div>
    </header>
  );
}
