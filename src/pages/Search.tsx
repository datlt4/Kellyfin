import { useSearchParams } from "@solidjs/router";
import { createResource, For, Show } from "solid-js";
import { api, type Card, type Lib } from "../api";
import { ItemCard, LibCard } from "../components/Card";

export default function Search() {
  const [sp, setSp] = useSearchParams();
  const q = () => (typeof sp.q === "string" ? sp.q : "");
  const [res] = createResource(q, (q) => (q.trim() ? api<{ libraries: Lib[]; items: Card[] }>(`/search?q=${encodeURIComponent(q)}`) : undefined));
  let timer: number | undefined;
  return (
    <div class="search">
      <div class="page-head">
        <input
          class="search-input"
          type="search"
          placeholder="Search movies and series"
          value={q()}
          autofocus
          onInput={(e) => {
            const v = e.currentTarget.value;
            clearTimeout(timer);
            timer = window.setTimeout(() => setSp({ q: v || undefined }, { replace: true }), 250);
          }}
        />
      </div>
      <Show when={res()}>
        {(r) => (
          <>
            <Show when={r().libraries.length}>
              <h2 class="section-title">Libraries</h2>
              <div class="grid"><For each={r().libraries}>{(l) => <LibCard lib={l} />}</For></div>
            </Show>
            <Show when={r().items.length}>
              <h2 class="section-title">Videos</h2>
              <div class="grid"><For each={r().items}>{(it) => <ItemCard item={it} />}</For></div>
            </Show>
            <Show when={!r().libraries.length && !r().items.length}><p class="muted pad">Nothing found for “{q()}”.</p></Show>
          </>
        )}
      </Show>
    </div>
  );
}
