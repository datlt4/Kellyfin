import { A, useParams } from "@solidjs/router";
import { createResource, For, Show } from "solid-js";
import { api, fmtDuration, fmtRes, img, user, type ItemDetail, type Stream } from "../api";

const label = (s: Stream) =>
  [s.title, s.lang && s.lang !== "und" ? s.lang : null, s.codec?.toUpperCase(), s.channels ? `${s.channels}ch` : null]
    .filter(Boolean)
    .join(" · ");

export default function Item() {
  const params = useParams();
  const [item] = createResource(() => params.id, (id) => api<ItemDetail>(`/items/${id}`));
  const of = (t: string) => item()?.streams?.filter((s) => s.type === t) ?? [];
  return (
    <Show when={item()} fallback={<div class="page-loading" />}>
      {(it) => (
        <div class="detail">
          <div class="backdrop" style={{ "background-color": it().color ?? undefined }}>
            <Show when={img(it(), "l")}>{(src) => <img src={src()} alt="" />}</Show>
          </div>
          <div class="detail-body">
            <A href={`/library/${it().library_id}`} class="eyebrow">{it().library_name}</A>
            <h1>{it().title}</h1>
            <div class="facts">
              <span>{fmtDuration(it().duration)}</span>
              <span>{fmtRes(it().height, it().width)} {it().video_codec?.toUpperCase()}</span>
              <span>{(it().size / 1048576).toFixed(0)} MB</span>
            </div>
            <div class="actions">
              <button class="primary" disabled title="Playback arrives in P2">▶ Play</button>
            </div>
            <dl class="tracks">
              <dt>Audio</dt>
              <dd>
                <select disabled={!of("audio").length}>
                  <For each={of("audio")} fallback={<option>None</option>}>{(s) => <option>{label(s)}</option>}</For>
                </select>
              </dd>
              <dt>Subtitles</dt>
              <dd>
                <select>
                  <option>Off</option>
                  <For each={of("subtitle")}>{(s) => <option>{label(s)}</option>}</For>
                </select>
              </dd>
            </dl>
            <Show when={user()?.is_admin}><p class="path">{it().path}</p></Show>
          </div>
        </div>
      )}
    </Show>
  );
}
