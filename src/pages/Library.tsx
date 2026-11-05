import { A, useParams } from "@solidjs/router";
import { createEffect, createResource, createSignal, For, on, onCleanup, Show } from "solid-js";
import { api, checkNarration, fmtDuration, fmtRes, img, pickPreset, user, type Card, type Lib } from "../api";
import { ItemCard } from "../components/Card";
import Icon from "../components/Icon";
import Select from "../components/Select";
import { notify } from "../components/Toast";

const PAGE = 120;

type View = "grid" | "list" | "tree";
type TreeNode = { name: string; path: string; folders: TreeNode[]; items: Card[] };
const savedView = (): View => {
  try {
    const v = localStorage.getItem("libView");
    // "tree-grid" was the grid variant of a separate tree list until 2.8.1; both are one tree view now.
    return v === "list" ? "list" : v === "tree" || v === "tree-grid" ? "tree" : "grid";
  } catch {
    return "grid";
  }
};
const isTree = (v: View) => v === "tree";
/** Videos in a branch, the folder itself and everything under it. */
const countVideos = (n: TreeNode | null): number => (n ? n.items.length + n.folders.reduce((m, f) => m + countVideos(f), 0) : 0);

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

/** One folder and everything under it; subfolders nest to any depth, the videos are grid cards. */
function TreeFolder(props: { node: TreeNode; closed: () => Set<string>; onToggle: (path: string) => void }) {
  const isClosed = () => props.closed().has(props.node.path);
  return (
    <section class="tree-folder">
      <button class="tree-head" onClick={() => props.onToggle(props.node.path)} aria-expanded={!isClosed()}>
        <span class="chevron" classList={{ open: !isClosed() }}><Icon name="right" size={17} /></span>
        <Icon name="tree" size={24} />
        <b>{props.node.name}</b>
        <span class="muted small">{countVideos(props.node)} videos</span>
      </button>
      <Show when={!isClosed()}>
        <div class="tree-children">
          <For each={props.node.folders}>{(f) => <TreeFolder node={f} closed={props.closed} onToggle={props.onToggle} />}</For>
          <Show when={props.node.items.length > 0}>
            <div class="grid"><For each={props.node.items}>{(it) => <ItemCard item={it} />}</For></div>
          </Show>
        </div>
      </Show>
    </section>
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

  const [tree, setTree] = createSignal<{ id: string; root: TreeNode } | null>(null);
  const [closed, setClosed] = createSignal<Set<string>>(new Set());
  const toggle = (p: string) => setClosed((c) => { const x = new Set(c); x.has(p) ? x.delete(p) : x.add(p); return x; });
  createEffect(on([() => params.id, () => isTree(view())], () => {
    // Keyed on entering tree mode, not on the grid/list/tree value, so folding state survives leaving and coming back.
    if (!isTree(view()) || !params.id) return;
    const id = params.id;
    if (tree()?.id === id) return; // already loaded for this library
    setTree(null);
    setClosed(new Set<string>());
    api<TreeNode>(`/libraries/${id}/tree`).then((t) => id === params.id && setTree({ id, root: t }));
  }));

  const load = async () => {
    if (isTree(view())) return;
    const g = gen;
    if (total() >= 0 && items().length >= total()) return;
    const r = await api<{ total: number; items: Card[] }>(
      `/libraries/${params.id}/items?offset=${items().length}&limit=${PAGE}&sort=${sort()}`,
    );
    if (g !== gen) return; // library or sort changed meanwhile
    setTotal(r.total);
    setItems([...items(), ...r.items]);
  };
  createEffect(on([() => params.id, sort, () => isTree(view())], () => {
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
          <span class="muted"><Show when={isTree(view()) ? tree() : total() >= 0}>{isTree(view()) ? countVideos(tree()?.root ?? null) : total()} videos</Show></span>
        </div>
        <div class="head-tools">
          <Show when={user()?.is_admin && lib()}>
            <button class="btn small" onClick={() => narrate().catch((e) => notify(e.message, false))} title="Vietnamese and Chinese voice-over for every video">Narration</button>
          </Show>
          <Show when={!isTree(view())}><Select value={sort()} onChange={(v) => setSort(v as "title" | "added")}
            options={[{ value: "title", label: "A – Z" }, { value: "added", label: "Recently added" }]} /></Show>
          {/* One button: shows the current view, a click moves to the next one. */}
          <button
            class="icon view-btn"
            onClick={() => setView(view() === "grid" ? "list" : view() === "list" ? "tree" : "grid")}
            aria-label={`${view()} view (click for the next one)`}
            title={view() === "grid" ? "Grid view" : view() === "list" ? "List view" : "Folder tree"}
          >
            <Icon name={view() === "tree" ? "tree-grid" : view()} size={20} />
          </button>
        </div>
      </div>
      <Show when={isTree(view())}>
        <div class="tree">
          {/* Videos directly in the library root first, then the folder tree. */}
          <Show when={tree()?.root.items.length}>
            <div class="tree-root">
              <div class="grid"><For each={tree()!.root.items}>{(it) => <ItemCard item={it} />}</For></div>
            </div>
          </Show>
          <For each={tree()?.root.folders ?? []}>
            {(f) => <TreeFolder node={f} closed={closed} onToggle={toggle} />}
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
