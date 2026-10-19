import { A } from "@solidjs/router";
import { Show } from "solid-js";
import { fmtDuration, img, type Card as C, type Lib } from "../api";
import Icon from "./Icon";

export function ItemCard(props: { item: C; sub?: string }) {
  const it = () => props.item;
  const pct = () => (it().duration && it().position ? Math.min(100, (it().position / it().duration!) * 100) : 0);
  return (
    <A href={`/item/${it().id}`} class="card">
      <div class="thumb" style={{ "background-color": it().color ?? undefined }}>
        <Show when={img(it(), "s")}>{(src) => <img src={src()} alt="" loading="lazy" decoding="async" />}</Show>
        <Show when={it().played}><span class="badge"><Icon name="check" size={14} /></span></Show>
        <Show when={pct() > 0}><div class="progress"><div style={{ width: `${pct()}%` }} /></div></Show>
      </div>
      <div class="card-title" title={it().title}>{it().title}</div>
      <div class="card-sub">{props.sub ?? fmtDuration(it().duration)}</div>
    </A>
  );
}

export function LibCard(props: { lib: Lib }) {
  return (
    <A href={`/library/${props.lib.id}`} class="card lib">
      <div class="thumb" style={{ "background-color": props.lib.color ?? undefined }}>
        <Show when={img(props.lib, "s")}>{(src) => <img src={src()} alt="" loading="lazy" decoding="async" />}</Show>
      </div>
      <div class="card-title" title={props.lib.name}>{props.lib.name}</div>
      <div class="card-sub">{props.lib.count} videos</div>
    </A>
  );
}
