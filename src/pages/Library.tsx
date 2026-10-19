import { useParams } from "@solidjs/router";
import { createEffect, createResource, createSignal, For, on, onCleanup, Show } from "solid-js";
import { api, type Card, type Lib } from "../api";
import { ItemCard } from "../components/Card";

const PAGE = 120;

export default function Library() {
  const params = useParams();
  const [sort, setSort] = createSignal<"title" | "added">("title");
  const [lib] = createResource(() => params.id, (id) => api<Lib>(`/libraries/${id}`));
  const [items, setItems] = createSignal<Card[]>([]);
  const [total, setTotal] = createSignal(-1);
  let gen = 0;

  const load = async () => {
    const g = gen;
    if (total() >= 0 && items().length >= total()) return;
    const r = await api<{ total: number; items: Card[] }>(
      `/libraries/${params.id}/items?offset=${items().length}&limit=${PAGE}&sort=${sort()}`,
    );
    if (g !== gen) return; // library or sort changed meanwhile
    setTotal(r.total);
    setItems([...items(), ...r.items]);
  };
  createEffect(on([() => params.id, sort], () => {
    gen++;
    setItems([]);
    setTotal(-1);
    load();
  }));

  let busy = false;
  const sentinel = (el: HTMLElement) => {
    const io = new IntersectionObserver(async (e) => {
      if (!e[0].isIntersecting || busy || total() < 0) return;
      busy = true;
      await load();
      busy = false;
    }, { rootMargin: "1200px" });
    io.observe(el);
    onCleanup(() => io.disconnect());
  };

  return (
    <div class="library">
      <div class="page-head">
        <div>
          <h1>{lib()?.name}</h1>
          <span class="muted"><Show when={total() >= 0}>{total()} videos</Show></span>
        </div>
        <select value={sort()} onChange={(e) => setSort(e.currentTarget.value as "title" | "added")}>
          <option value="title">A – Z</option>
          <option value="added">Recently added</option>
        </select>
      </div>
      <div class="grid">
        <For each={items()}>{(it) => <ItemCard item={it} />}</For>
      </div>
      <div ref={sentinel} class="sentinel" />
    </div>
  );
}
