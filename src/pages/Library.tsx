import { A, useParams } from "@solidjs/router";
import { createEffect, createResource, createSignal, For, on, onCleanup, Show } from "solid-js";
import { api, checkNarration, fmtDuration, fmtRes, img, pickPreset, user, type Card, type Lib } from "../api";
import { ItemCard } from "../components/Card";
import Icon from "../components/Icon";
import Select from "../components/Select";
import { notify } from "../components/Toast";

const PAGE = 120;

type View = "grid" | "list" | "tree";
type Folder = { name: string; items: Card[] };
const savedView = (): View => {
  try {
    const v = localStorage.getItem("libView");
    return v === "list" || v === "tree" ? v : "grid";
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

  const [folders, setFolders] = createSignal<Folder[]>([]);
  const [closed, setClosed] = createSignal<Set<string>>(new Set());
  const toggle = (n: string) => setClosed((c) => { const x = new Set(c); x.has(n) ? x.delete(n) : x.add(n); return x; });
  createEffect(on([() => params.id, view], () => {
    if (view() !== "tree") return;
    const id = params.id;
    setFolders([]);
    setClosed(new Set<string>());
    api<Folder[]>(`/libraries/${id}/tree`).then((f) => id === params.id && setFolders(f));
  }));

  const load = async () => {
    if (view() === "tree") return;
    const g = gen;
    if (total() >= 0 && items().length >= total()) return;
    const r = await api<{ total: number; items: Card[] }>(
      `/libraries/${params.id}/items?offset=${items().length}&limit=${PAGE}&sort=${sort()}`,
    );
    if (g !== gen) return; // library or sort changed meanwhile
    setTotal(r.total);
    setItems([...items(), ...r.items]);
  };
  createEffect(on([() => params.id, sort, () => view() === "tree"], () => {
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

  // Admins: queue every video of this library for narration (Settings → Narration shows the progress).
  async function narrate() {
    const preset = await pickPreset();
    if (preset === null) return;
    await checkNarration(preset.graph);
    const n = await api<number>("/admin/narration/jobs", { method: "POST", body: JSON.stringify({ library_id: lib()!.id, preset: preset.id }) });
    notify(n ? `${n} videos queued. Progress: Settings → Narration.` : "Nothing queued: every video of this library is already waiting, running or paused.");
  }

  return (
    <div class="library">
      <div class="page-head">
        <div>
          <h1>{lib()?.name}</h1>
          <span class="muted"><Show when={view() === "tree" ? folders().length : total() >= 0}>{view() === "tree" ? folders().reduce((n, f) => n + f.items.length, 0) : total()} videos</Show></span>
        </div>
        <div class="head-tools">
          <Show when={user()?.is_admin && lib()}>
            <button class="btn small" onClick={() => narrate().catch((e) => notify(e.message, false))} title="Vietnamese and Chinese voice-over for every video">Narration</button>
          </Show>
          <Show when={view() !== "tree"}><Select value={sort()} onChange={(v) => setSort(v as "title" | "added")}
            options={[{ value: "title", label: "A – Z" }, { value: "added", label: "Recently added" }]} /></Show>
          {/* One button: shows the current view, a click moves to the next one. */}
          <button
            class="icon view-btn"
            onClick={() => setView(view() === "grid" ? "list" : view() === "list" ? "tree" : "grid")}
            aria-label={`${view()} view (click for the next one)`}
            title={view() === "grid" ? "Grid view" : view() === "list" ? "List view" : "Folder tree view"}
          >
            <Icon name={view()} size={20} />
          </button>
        </div>
      </div>
      <Show when={view() === "tree"}>
        <div class="tree">
          <For each={folders()}>
            {(f) => (
              <section>
                <button class="folder-head" onClick={() => toggle(f.name)} aria-expanded={!closed().has(f.name)}>
                  <Icon name="tree" size={18} />
                  <b>{f.name || "(this folder)"}</b>
                  <span class="muted small">{f.items.length} videos</span>
                </button>
                <Show when={!closed().has(f.name)}>
                  <div class="rows"><For each={f.items}>{(it) => <ItemRow item={it} />}</For></div>
                </Show>
              </section>
            )}
          </For>
        </div>
      </Show>
      <Show when={view() === "grid"}>
        <div class="grid"><For each={items()}>{(it) => <ItemCard item={it} />}</For></div>
      </Show>
      <Show when={view() === "list"}>
        <div class="rows"><For each={items()}>{(it) => <ItemRow item={it} />}</For></div>
      </Show>
      <div ref={sentinel} class="sentinel" />
    </div>
  );
}
