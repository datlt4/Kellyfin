import { A } from "@solidjs/router";
import { Show, type JSX } from "solid-js";
import Icon from "./Icon";

/** Horizontally scrolling shelf, Jellyfin-style. */
export default function Row(props: { title: string; href?: string; children: JSX.Element }) {
  let scroller!: HTMLDivElement;
  const page = (dir: number) => scroller.scrollBy({ left: dir * scroller.clientWidth * 0.9, behavior: "smooth" });
  return (
    <section class="row">
      <div class="row-head">
        <Show when={props.href} fallback={<h2>{props.title}</h2>}>
          <A href={props.href!}><h2>{props.title} ›</h2></A>
        </Show>
        <div class="row-nav">
          <button onClick={() => page(-1)} aria-label="Previous"><Icon name="left" size={20} /></button>
          <button onClick={() => page(1)} aria-label="Next"><Icon name="right" size={20} /></button>
        </div>
      </div>
      <div class="scroller" ref={scroller}>{props.children}</div>
    </section>
  );
}
