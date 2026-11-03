import { createMemo, createSignal, For, onCleanup, Show } from "solid-js";

/** A drop-down with a filter box (accent-insensitive), in place of <select>. Opens below its button in a fixed
 *  layer, so scrolled or clipped containers (the pipeline canvas) do not cut it off. */

export type Option = { value: string; label: string; disabled?: boolean };

const fold = (s: string) => s.normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/đ/gi, "d").toLowerCase();

export default function Select(props: {
  value: string;
  options: Option[];
  onChange: (v: string) => void;
  disabled?: boolean;
  placeholder?: string;
  /** Submitted with a <form> under this name. */
  name?: string;
  class?: string;
}) {
  const [open, setOpen] = createSignal(false);
  const [q, setQ] = createSignal("");
  const [hi, setHi] = createSignal(0);
  const [pos, setPos] = createSignal({ left: 0, top: 0, width: 0, up: false });
  let button!: HTMLButtonElement;
  let filter: HTMLInputElement | undefined;
  const current = () => props.options.find((o) => o.value === props.value);
  const shown = createMemo(() => {
    const f = fold(q().trim());
    return f ? props.options.filter((o) => fold(o.label).includes(f) || fold(o.value).includes(f)) : props.options;
  });

  const close = () => {
    setOpen(false);
    setQ("");
  };
  const onOutside = (e: PointerEvent) => {
    if (!(e.target as HTMLElement).closest(".sel-pop") && e.target !== button && !button.contains(e.target as Node)) close();
  };
  const onScroll = (e: Event) => {
    if (!(e.target instanceof Node && e.target.parentElement?.closest(".sel-pop"))) close();
  };
  function show() {
    if (props.disabled) return;
    const r = button.getBoundingClientRect();
    const up = r.bottom + 320 > innerHeight && r.top > 320;
    setPos({ left: r.left, top: up ? r.top : r.bottom + 4, width: Math.max(r.width, 220), up });
    setHi(Math.max(0, shown().findIndex((o) => o.value === props.value)));
    setOpen(true);
    queueMicrotask(() => filter?.focus());
    document.addEventListener("pointerdown", onOutside, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", close);
  }
  onCleanup(() => {
    document.removeEventListener("pointerdown", onOutside, true);
    window.removeEventListener("scroll", onScroll, true);
    window.removeEventListener("resize", close);
  });
  const pick = (o: Option | undefined) => {
    if (!o || o.disabled) return;
    props.onChange(o.value);
    close();
    button.focus();
  };
  function key(e: KeyboardEvent) {
    const n = shown().length;
    if (e.key === "ArrowDown") setHi((hi() + 1) % Math.max(1, n));
    else if (e.key === "ArrowUp") setHi((hi() - 1 + n) % Math.max(1, n));
    else if (e.key === "Enter") pick(shown()[hi()]);
    else if (e.key === "Escape") {
      close();
      button.focus();
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }

  return (
    <>
      <button
        type="button"
        ref={button}
        class={`sel ${props.class ?? ""}`}
        disabled={props.disabled}
        aria-haspopup="listbox"
        aria-expanded={open()}
        onClick={() => (open() ? close() : show())}
        onKeyDown={(e) => {
          if (!open() && (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            show();
          }
        }}
      >
        <span class="sel-label">{current()?.label ?? props.placeholder ?? "Choose…"}</span>
        <span class="sel-caret">▾</span>
      </button>
      <Show when={props.name}><input type="hidden" name={props.name} value={props.value} /></Show>
      <Show when={open()}>
        <div
          class="sel-pop"
          style={{
            left: `${Math.min(pos().left, innerWidth - pos().width - 8)}px`,
            width: `${pos().width}px`,
            ...(pos().up ? { bottom: `${innerHeight - pos().top + 4}px` } : { top: `${pos().top}px` }),
          }}
        >
          <input
            ref={filter}
            class="sel-filter"
            placeholder="Filter…"
            value={q()}
            onInput={(e) => {
              setQ(e.currentTarget.value);
              setHi(0);
            }}
            onKeyDown={key}
          />
          <div class="sel-list" role="listbox">
            <For each={shown()} fallback={<div class="sel-empty muted small">No match</div>}>
              {(o, i) => (
                <div
                  role="option"
                  aria-selected={o.value === props.value}
                  class="sel-opt"
                  classList={{ hi: i() === hi(), on: o.value === props.value, off: o.disabled }}
                  onPointerEnter={() => setHi(i())}
                  onClick={() => pick(o)}
                >
                  {o.label}
                </div>
              )}
            </For>
          </div>
        </div>
      </Show>
    </>
  );
}
