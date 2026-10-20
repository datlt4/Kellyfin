import { A } from "@solidjs/router";
import { createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { api, fmtDuration, img, type Card, type Home as H, type LatestRow } from "../api";
import { ItemCard, LibCard } from "../components/Card";
import Row from "../components/Row";

function Hero(props: { item: Card; label: string }) {
  return (
    <A href={`/item/${props.item.id}`} class="hero" style={{ "background-color": props.item.color ?? undefined }}>
      <Show when={img(props.item, "l")}>{(src) => <img src={src()} alt="" />}</Show>
      <div class="hero-text">
        <span class="eyebrow">{props.label}</span>
        <h1>{props.item.title}</h1>
        <span class="muted">{fmtDuration(props.item.duration)}</span>
      </div>
    </A>
  );
}

export default function Home() {
  const [home] = createResource(() => api<H>("/home"));
  // "Recently added" shelves beyond the first few load as the user scrolls.
  const [more, setMore] = createSignal<LatestRow[]>([]);
  const [done, setDone] = createSignal(false);
  let loading = false;
  const loadMore = async () => {
    if (loading || done() || !home()) return;
    loading = true;
    const rows = await api<LatestRow[]>(`/home/latest?offset=${home()!.latest.length + more().length}&limit=6`);
    setMore([...more(), ...rows]);
    if (rows.length < 6) setDone(true);
    loading = false;
  };
  const sentinel = (el: HTMLElement) => {
    const io = new IntersectionObserver((e) => e[0].isIntersecting && loadMore(), { rootMargin: "800px" });
    io.observe(el);
    onCleanup(() => io.disconnect());
  };
  const hero = () => home()?.resume[0] ?? home()?.latest[0]?.items[0];

  return (
    <Show
      when={!home.error}
      fallback={
        <div class="page-error">
          <p class="error">Cannot load: {String(home.error?.message ?? home.error)}</p>
          <button class="btn" onClick={() => location.reload()}>Retry</button>
        </div>
      }
    >
      <Show when={home()} fallback={<div class="page-loading" />}>
        {(h) => (
          <div class="home">
            <Show when={hero()}>{(it) => <Hero item={it()} label={h().resume.length ? "Continue watching" : "Recently added"} />}</Show>
            <Row title="My Media">
              <For each={h().libraries}>{(l) => <LibCard lib={l} />}</For>
            </Row>
            <Show when={h().resume.length}>
              <Row title="Continue Watching">
                <For each={h().resume}>{(it) => <ItemCard item={it} />}</For>
              </Row>
            </Show>
            <For each={[...h().latest, ...more()]}>
              {(r) => (
                <Row title={`Recently Added in ${r.name}`} href={`/library/${r.id}`}>
                  <For each={r.items}>{(it) => <ItemCard item={it} />}</For>
                </Row>
              )}
            </For>
            <Show when={!done()}><div ref={sentinel} class="sentinel" /></Show>
          </div>
        )}
      </Show>
    </Show>
  );
}
