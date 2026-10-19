import { A, useParams } from "@solidjs/router";
import { createEffect, createResource, createSignal, For, on, Show } from "solid-js";
import { api, fmtClock, fmtDuration, fmtRes, img, streamLabel, subLabel, upload, user, type ItemDetail, type Related } from "../api";
import { ItemCard, LibCard } from "../components/Card";
import Row from "../components/Row";
import { audioStreams, defaultAudio, defaultSub } from "../playback";

export default function Item() {
  const params = useParams();
  const [item, { mutate, refetch }] = createResource(() => params.id, (id) => api<ItemDetail>(`/items/${id}`));
  const [related] = createResource(() => params.id, (id) => api<Related>(`/items/${id}/related`));
  const [audio, setAudio] = createSignal<number | null>(null);
  const [sub, setSub] = createSignal("off");
  createEffect(on(item, (it) => {
    if (!it) return;
    setAudio(defaultAudio(it));
    setSub(defaultSub(it));
  }));

  const playHref = (fromStart = false) => {
    const q = new URLSearchParams({ sub: sub() });
    if (audio() !== null) q.set("audio", String(audio()));
    if (fromStart) q.set("t", "0");
    return `/play/${item()!.id}?${q}`;
  };
  const resumable = () => !!item() && !item()!.played && item()!.position > 5;

  async function rename() {
    const t = prompt("Title (empty = use the file name)", item()!.title);
    if (t === null) return;
    await api(`/admin/items/${item()!.id}`, { method: "PATCH", body: JSON.stringify({ title: t }) });
    refetch();
  }

  function changeImage() {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    input.onchange = async () => {
      const f = input.files?.[0];
      if (!f) return;
      await upload(`/admin/items/${item()!.id}/image`, f).catch((e) => alert(e.message));
      refetch();
    };
    input.click();
  }

  async function autoImage() {
    await api(`/admin/items/${item()!.id}/image`, { method: "DELETE" });
    refetch();
  }

  async function patch(body: { played?: boolean; favorite?: boolean }) {
    const it = item()!;
    mutate({ ...it, ...body, position: body.played !== undefined ? 0 : it.position });
    await api(`/items/${it.id}/userdata`, { method: "POST", body: JSON.stringify(body) }).catch(() => mutate(it));
  }

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
              <A class="btn primary" href={playHref()}>
                ▶ {resumable() ? `Resume ${fmtClock(it().position)}` : "Play"}
              </A>
              <Show when={resumable()}>
                <A class="btn" href={playHref(true)}>From start</A>
              </Show>
              <button class="btn round" classList={{ on: it().played }} onClick={() => patch({ played: !it().played })} title="Watched">✓</button>
              <button class="btn round" classList={{ on: it().favorite }} onClick={() => patch({ favorite: !it().favorite })} title="Favorite">♥</button>
            </div>
            <Show when={resumable()}>
              <div class="resume-bar"><div style={{ width: `${(it().position / (it().duration || 1)) * 100}%` }} /></div>
            </Show>
            <dl class="tracks">
              <dt>Audio</dt>
              <dd>
                <select value={audio() ?? ""} disabled={audioStreams(it()).length < 2} onChange={(e) => setAudio(Number(e.currentTarget.value))}>
                  <For each={audioStreams(it())} fallback={<option>None</option>}>{(s) => <option value={s.index}>{streamLabel(s)}</option>}</For>
                </select>
              </dd>
              <dt>Subtitles</dt>
              <dd>
                <select value={sub()} onChange={(e) => setSub(e.currentTarget.value)}>
                  <option value="off">Off</option>
                  <For each={it().subtitles}>{(s) => <option value={s.key} disabled={!s.supported}>{subLabel(s)}</option>}</For>
                </select>
              </dd>
            </dl>
            <div class="neighbours">
              <Show when={it().prev_id}><A href={`/item/${it().prev_id}`}>‹ Previous</A></Show>
              <Show when={it().next_id}><A href={`/item/${it().next_id}`}>Next ›</A></Show>
            </div>
            <Show when={user()?.is_admin}>
              <div class="row-btns admin-tools">
                <button class="btn small" onClick={rename}>Edit title</button>
                <button class="btn small" onClick={changeImage}>Change image</button>
                <button class="btn small" onClick={autoImage} title="Grab a frame from the video">Auto image</button>
              </div>
              <p class="path">{it().path}</p>
            </Show>
          </div>
          <Show when={related()?.libraries.length}>
            <Row title="More like this">
              <For each={related()!.libraries}>{(l) => <LibCard lib={l} />}</For>
            </Row>
          </Show>
          <Show when={related()?.items.length}>
            <Row title="Similar videos">
              <For each={related()!.items}>{(c) => <ItemCard item={c} />}</For>
            </Row>
          </Show>
        </div>
      )}
    </Show>
  );
}
