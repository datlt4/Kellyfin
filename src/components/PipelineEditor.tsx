import { createSignal, For, Index, onCleanup, Show } from "solid-js";
import type { PipeGraph, PipeNode } from "../api";
import Select from "./Select";

/** Narration pipeline canvas: drag nodes by their title, drag from a node's right dot to another's left dot to wire
 *  them, click a wire to remove it. The server checks the graph when it is saved or queued. */

export const PIPE_LANGS: [string, string][] = [
  ["en", "English"], ["vi", "Vietnamese"], ["zh", "Chinese"], ["ja", "Japanese"], ["ko", "Korean"], ["de", "German"],
  ["fr", "French"], ["es", "Spanish"], ["it", "Italian"], ["pt", "Portuguese"], ["ru", "Russian"], ["th", "Thai"],
  ["id", "Indonesian"], ["ms", "Malay"],
];

/** What the film-sub EasyOCR reads (server: pipeline::OCR_LANGS). */
const OCR_LANGS = ["vi", "en", "zh", "ja", "ko"];
const langName = (c: string) => PIPE_LANGS.find(([k]) => k === c)?.[1] ?? c;
const W = 210; // node width
const PORT_Y = 20; // port height from the node's top

const TITLES: Record<PipeNode["type"], string> = {
  whisper: "Whisper (speech → subtitles)",
  ocr: "OCR (burned-in subtitles)",
  subtitle: "Video's own subtitles",
  translate: "Translate (Argos)",
  voice: "Voice (TTS)",
  mux: "Mux into the video",
};
const SOURCES = ["whisper", "ocr", "subtitle"];
const makesSubs = (t: string) => SOURCES.includes(t) || t === "translate";
const takesInput = (t: string) => t === "translate" || t === "voice" || t === "mux";

/** Whether `from` may feed `to` (the server applies the same rules). */
function fits(from: PipeNode, to: PipeNode) {
  if (from.id === to.id) return false;
  if (to.type === "mux") return from.type === "voice";
  return (to.type === "translate" || to.type === "voice") && makesSubs(from.type);
}

export default function PipelineEditor(props: {
  graph: PipeGraph;
  onChange?: (g: PipeGraph) => void;
  /** VieNeu preset voices and edge-tts voice names, as the workers report them. */
  voices?: string[];
  edgeVoices?: string[];
}) {
  const ro = () => !props.onChange;
  const g = () => props.graph;
  const set = (next: PipeGraph) => props.onChange?.(next);
  const byId = (id: string) => g().nodes.find((n) => n.id === id);
  const patch = (id: string, p: Partial<PipeNode>) => set({ ...g(), nodes: g().nodes.map((n) => (n.id === id ? { ...n, ...p } : n)) });
  let board!: HTMLDivElement;
  const [wire, setWire] = createSignal<{ from: string; x: number; y: number } | null>(null);
  const [full, setFull] = createSignal(false);
  const onEsc = (e: KeyboardEvent) => e.key === "Escape" && !document.querySelector(".sel-pop") && setFull(false);
  window.addEventListener("keydown", onEsc);
  onCleanup(() => window.removeEventListener("keydown", onEsc));
  /** Language of the subtitles a node receives ("" when Whisper detects it). */
  const inputLang = (id: string): string => {
    const from = byId(g().edges.find(([, b]) => b === id)?.[0] ?? "");
    return !from || from.lang === "auto" ? "" : (from.lang ?? "");
  };

  const at = (e: PointerEvent) => {
    const r = board.getBoundingClientRect();
    return { x: e.clientX - r.left + board.scrollLeft, y: e.clientY - r.top + board.scrollTop };
  };

  function drag(n: PipeNode, e: PointerEvent) {
    if (ro() || (e.target as HTMLElement).closest("button")) return;
    const el = e.currentTarget as HTMLElement;
    const start = at(e), x0 = n.x, y0 = n.y;
    el.setPointerCapture(e.pointerId);
    el.onpointermove = (m) => {
      const p = at(m);
      patch(n.id, { x: Math.max(0, Math.round((x0 + p.x - start.x) / 10) * 10), y: Math.max(0, Math.round((y0 + p.y - start.y) / 10) * 10) });
    };
    el.onpointerup = el.onpointercancel = () => (el.onpointermove = el.onpointerup = el.onpointercancel = null);
  }

  function connect(n: PipeNode, e: PointerEvent) {
    if (ro()) return;
    e.stopPropagation();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    setWire({ from: n.id, ...at(e) });
    el.onpointermove = (m) => setWire({ from: n.id, ...at(m) });
    el.onpointerup = (u) => {
      el.onpointermove = el.onpointerup = el.onpointercancel = null;
      setWire(null);
      const target = (document.elementFromPoint(u.clientX, u.clientY) as HTMLElement | null)?.closest("[data-node]");
      const to = target && byId(target.getAttribute("data-node")!);
      if (!to || !fits(n, to)) return;
      // Translate and voice take one input: a new wire replaces the old one.
      const edges = g().edges.filter(([a, b]) => !(b === to.id && (to.type !== "mux" || a === n.id)));
      set({ ...g(), edges: [...edges, [n.id, to.id]] });
    };
    el.onpointercancel = () => {
      el.onpointermove = el.onpointerup = el.onpointercancel = null;
      setWire(null);
    };
  }

  function add(type: PipeNode["type"]) {
    let i = 1;
    while (byId(`${type}${i}`)) i++;
    const lang = type === "whisper" ? "auto" : type === "ocr" || type === "subtitle" || type === "translate" ? "vi" : undefined;
    const n: PipeNode = { id: `${type}${i}`, type, lang, x: 40 + (g().nodes.length % 5) * 30, y: 30 + (g().nodes.length % 5) * 30 };
    if (type === "voice") n.engine = "edge";
    if (type === "ocr") n.crop = 0.35;
    set({ ...g(), nodes: [...g().nodes, n] });
  }

  const remove = (id: string) => set({ nodes: g().nodes.filter((n) => n.id !== id), edges: g().edges.filter(([a, b]) => a !== id && b !== id) });
  const out = (n: PipeNode) => ({ x: n.x + W, y: n.y + PORT_Y });
  const inp = (n: PipeNode) => ({ x: n.x, y: n.y + PORT_Y });
  const curve = (a: { x: number; y: number }, b: { x: number; y: number }) =>
    `M ${a.x} ${a.y} C ${a.x + 70} ${a.y}, ${b.x - 70} ${b.y}, ${b.x} ${b.y}`;
  const extent = () => ({
    w: Math.max(900, ...g().nodes.map((n) => n.x + W + 60)),
    h: Math.max(340, ...g().nodes.map((n) => n.y + 180)),
  });

  const langSelect = (n: PipeNode, auto = false, only?: string[]) => (
    <Select value={n.lang ?? ""} disabled={ro()} onChange={(v) => patch(n.id, { lang: v })} options={[
      ...(auto ? [{ value: "auto", label: "Detect the language" }] : []),
      ...PIPE_LANGS.filter(([c]) => !only || only.includes(c)).map(([c, l]) => ({ value: c, label: l })),
    ]} />
  );
  const edgeFor = (n: PipeNode) => {
    const l = inputLang(n.id);
    return (props.edgeVoices ?? []).filter((v) => !l || v.startsWith(`${l}-`));
  };

  return (
    <div class="pipe" classList={{ full: full() }}>
      <div class="pipe-palette">
        <Show when={!ro()}>
          <span class="muted small">Add:</span>
          <For each={["whisper", "ocr", "subtitle", "translate", "voice", "mux"] as PipeNode["type"][]}>
            {(t) => <button type="button" class="btn small" onClick={() => add(t)}>{TITLES[t].split(" (")[0]}</button>}
          </For>
        </Show>
        <span class="grow" />
        <button type="button" class="btn small" onClick={() => setFull(!full())}>{full() ? "Close full screen (Esc)" : "⛶ Full screen"}</button>
      </div>
      <div class="pipe-board" ref={board}>
        <div class="pipe-canvas" style={{ width: `${extent().w}px`, height: `${extent().h}px` }}>
          <svg class="pipe-wires" width={extent().w} height={extent().h}>
            <For each={g().edges}>
              {([a, b]) => {
                const from = () => byId(a), to = () => byId(b);
                return (
                  <Show when={from() && to()}>
                    <path class="pipe-wire" d={curve(out(from()!), inp(to()!))} />
                    <Show when={!ro()}>
                      <path class="pipe-hit" d={curve(out(from()!), inp(to()!))} onClick={() => set({ ...g(), edges: g().edges.filter(([x, y]) => !(x === a && y === b)) })}>
                        <title>Click to remove this wire</title>
                      </path>
                    </Show>
                  </Show>
                );
              }}
            </For>
            <Show when={wire()}>{(w) => <path class="pipe-wire pending" d={curve(out(byId(w().from)!), w())} />}</Show>
          </svg>
          {/* Index, not For: a moved node is a new object, and its element must stay (it holds the drag). */}
          <Index each={g().nodes}>
            {(n) => (
              <div class={`pipe-node ${n().type}`} style={{ left: `${n().x}px`, top: `${n().y}px`, width: `${W}px` }} data-node={n().id}>
                <div class="pipe-head" onPointerDown={(e) => drag(n(), e)}>
                  <span>{TITLES[n().type]}</span>
                  <Show when={!ro()}><button type="button" class="pipe-x" onClick={() => remove(n().id)} aria-label="Remove node">×</button></Show>
                </div>
                <Show when={takesInput(n().type)}><span class="pipe-port in" data-node={n().id} title="Input" /></Show>
                <Show when={n().type !== "mux"}><span class="pipe-port out" onPointerDown={(e) => connect(n(), e)} title="Drag to a node's input" /></Show>
                <div class="pipe-body">
                  <Show when={n().type === "whisper"}>{langSelect(n(), true)}</Show>
                  <Show when={n().type === "subtitle" || n().type === "translate"}>{langSelect(n())}</Show>
                  <Show when={n().type === "ocr"}>
                    {langSelect(n(), false, OCR_LANGS)}
                    <div class="pipe-also">
                      <span class="muted">Also read:</span>
                      <For each={OCR_LANGS.filter((l) => l !== n().lang)}>
                        {(l) => (
                          <label class="check">
                            <input type="checkbox" disabled={ro()} checked={(n().also ?? []).includes(l)}
                              onChange={(e) => patch(n().id, { also: e.currentTarget.checked ? [...(n().also ?? []), l] : (n().also ?? []).filter((x) => x !== l) })} />
                            {langName(l)}
                          </label>
                        )}
                      </For>
                    </div>
                    <label class="pipe-inline">
                      Bottom
                      <input type="number" min="5" max="80" step="5" value={Math.round((n().crop ?? 0.35) * 100)} disabled={ro()}
                        onChange={(e) => patch(n().id, { crop: Number(e.currentTarget.value) / 100 })} />
                      % of the picture
                    </label>
                  </Show>
                  <Show when={n().type === "voice"}>
                    <Select value={n().engine ?? "edge"} disabled={ro()} onChange={(v) => patch(n().id, { engine: v, voice: "" })} options={[
                      { value: "vieneu", label: "VieNeu (Vietnamese)" },
                      { value: "edge", label: "edge-tts (any language)" },
                    ]} />
                    <Show
                      when={(n().engine === "vieneu" ? props.voices : edgeFor(n()))?.length}
                      fallback={<input value={n().voice ?? ""} disabled={ro()} placeholder="default voice" onChange={(e) => patch(n().id, { voice: e.currentTarget.value.trim() })} />}
                    >
                      <Select value={n().voice ?? ""} disabled={ro()} onChange={(v) => patch(n().id, { voice: v })} options={[
                        { value: "", label: n().engine === "vieneu" ? "Default (Settings)" : "Default for the language" },
                        ...((n().engine === "vieneu" ? props.voices : edgeFor(n())) ?? []).map((v) => ({ value: v, label: v })),
                      ]} />
                    </Show>
                  </Show>
                  <Show when={n().type === "mux"}><span class="muted small">One audio track per voice, over the ducked original sound</span></Show>
                </div>
              </div>
            )}
          </Index>
        </div>
      </div>
    </div>
  );
}
