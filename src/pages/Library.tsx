import { A, useParams } from "@solidjs/router";
import { createEffect, createResource, createSignal, For, on, onCleanup, Show } from "solid-js";
import { api, fmtDuration, fmtRes, img, type Card, type Lib } from "../api";
import { ItemCard } from "../components/Card";
import Icon from "../components/Icon";

const PAGE = 120;

type View = "grid" | "list";
const savedView = (): View => {
  try {
    return localStorage.getItem("libView") === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
};

/** Compact row: small thumbnail, title, length, progress. */
function ItemRow(props: { item: Card }) {
  const it = () => props.item;
  const pct = () => (it().duration && it().position ? Math.min(100, (it().position / it().duration!) * 100) : 0);
  return (
    <A href={`/item/${it().id}`} class="row-item">
      <div class="thumb" style={{ "background-color": it().color ?? undefined }}>
        <Show when={img(it(), "s")}>{(src) => <img src={src()} alt="" loading="lazy" decoding="async" />}</Show>
        <Show when={pct() > 0}><div class="progress"><div style={{ width: `${pct()}%` }} /></div></Show>
      </div>
      <div class="row-text">
        <b title={it().title}>{it().title}</b>
        <span class="muted small">{[fmtDuration(it().duration), fmtRes(it().height, it().width)].filter(Boolean).join(" · ")}</span>
      </div>
      <Show when={it().played}><span class="badge static"><Icon name="check" size={14} /></span></Show>
    </A>
  );
}

export default function Library() {
  const params = useParams();
  const [sort, setSort] = createSignal<"title" | "added">("title");
  const [view, setViewSignal] = createSignal<View>(savedView());
  const setView = (v: View) => {
    setViewSignal(v);
    try {
      localStorage.setItem("libView", v);
    } catch {}
  };
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
        <div class="head-tools">
          <select value={sort()} onChange={(e) => setSort(e.currentTarget.value as "title" | "added")}>
            <option value="title">A – Z</option>
            <option value="added">Recently added</option>
          </select>
          {/* One button: shows the current view, a click moves to the next one. */}
          <button
            class="icon view-btn"
            onClick={() => setView(view() === "grid" ? "list" : "grid")}
            aria-label={view() === "grid" ? "Grid view (switch to list)" : "List view (switch to grid)"}
            title={view() === "grid" ? "Grid view" : "List view"}
          >
            <Icon name={view()} size={20} />
          </button>
        </div>
      </div>
      <Show
        when={view() === "list"}
        fallback={<div class="grid"><For each={items()}>{(it) => <ItemCard item={it} />}</For></div>}
      >
        <div class="rows"><For each={items()}>{(it) => <ItemRow item={it} />}</For></div>
      </Show>
      <div ref={sentinel} class="sentinel" />
    </div>
  );
}
