import { createResource, createSignal, For, Show } from "solid-js";
import { api, type FsList } from "../api";

/** Browses the server's folders (as the container sees them) and returns the chosen path. */
export default function FolderPicker(props: { start?: string; onPick: (path: string) => void; onClose: () => void }) {
  const [path, setPath] = createSignal(props.start ?? "/");
  const [list] = createResource(path, (p) => api<FsList>(`/admin/fs?path=${encodeURIComponent(p)}`));
  const join = (dir: string) => `${path().replace(/\/$/, "")}/${dir}`;
  return (
    <div class="modal" onClick={(e) => e.target === e.currentTarget && props.onClose()}>
      <div class="modal-box">
        <h3>Choose a folder</h3>
        <div class="crumb">{path()}</div>
        <div class="fs-list">
          <Show when={list()?.parent}>{(p) => <button type="button" onClick={() => setPath(p())}>↰ ..</button>}</Show>
          <For each={list()?.dirs} fallback={<p class="muted">{list.error ? String(list.error) : "No subfolders"}</p>}>
            {(d) => <button type="button" onClick={() => setPath(join(d))}>📁 {d}</button>}
          </For>
        </div>
        <div class="modal-actions">
          <button type="button" class="btn" onClick={props.onClose}>Cancel</button>
          <button type="button" class="btn primary" onClick={() => props.onPick(path())}>Use this folder</button>
        </div>
      </div>
    </div>
  );
}
