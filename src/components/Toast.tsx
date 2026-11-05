import { createSignal, Show } from "solid-js";

const [toast, setToast] = createSignal<{ ok: boolean; text: string } | null>(null);
let timer: number | undefined;

/** A message at the bottom of the screen (an alert() is swallowed by some app webviews). Errors stay longer. */
export function notify(text: string, ok = true) {
  setToast({ ok, text });
  clearTimeout(timer);
  timer = window.setTimeout(() => setToast(null), ok ? 6000 : 15000);
}

export default function Toast() {
  return (
    <Show when={toast()}>
      {(t) => (
        <div class="toast" classList={{ bad: !t().ok }} role="status" onClick={() => setToast(null)}>
          {t().text}
        </div>
      )}
    </Show>
  );
}
