import { A, useParams } from "@solidjs/router";
import { createResource, createSignal, For, Match, onCleanup, Show, Switch } from "solid-js";
import {
  api, ApiError, fmtClock, img, refreshUser, upload, user,
  type Info, type Lib, type LibRow, type ScanStatus, type UserRow,
} from "../api";
import FolderPicker from "../components/FolderPicker";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Runs an action and shows its error (or a short confirmation) next to the button. */
function useAction() {
  const [note, setNote] = createSignal<{ ok: boolean; text: string } | null>(null);
  const run = async (f: () => Promise<unknown>, done?: string) => {
    setNote(null);
    try {
      await f();
      if (done) setNote({ ok: true, text: done });
      return true;
    } catch (e) {
      setNote({ ok: false, text: msg(e) });
      return false;
    }
  };
  const Note = () => <Show when={note()}>{(n) => <p class={n().ok ? "ok" : "error"}>{n().text}</p>}</Show>;
  return { run, Note };
}

function pickFile(accept: string): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.onchange = () => resolve(input.files?.[0] ?? null);
    input.click();
  });
}

// ---------- Profile ----------

function Profile() {
  const [cur, setCur] = createSignal("");
  const [pw, setPw] = createSignal("");
  const [pw2, setPw2] = createSignal("");
  const pwAct = useAction();
  const avAct = useAction();
  const changeAvatar = async () => {
    const f = await pickFile("image/*");
    if (f) avAct.run(async () => { await upload("/me/avatar", f); await refreshUser(); });
  };
  const savePw = (e: Event) => {
    e.preventDefault();
    pwAct.run(async () => {
      if (pw() !== pw2()) throw new Error("New passwords do not match");
      await api("/me/password", { method: "POST", body: JSON.stringify({ current: cur(), new: pw() }) });
      setCur(""); setPw(""); setPw2("");
    }, "Password changed");
  };
  return (
    <>
      <section class="panel profile">
        <div class="avatar-big">
          <Show when={user()?.img} fallback={<span>{user()?.name.slice(0, 1).toUpperCase()}</span>}>
            <img src={img({ img: user()!.img!, imgv: user()!.imgv ?? 0 }, "s")} alt="" />
          </Show>
        </div>
        <div>
          <h2>{user()?.name}</h2>
          <div class="row-btns">
            <button class="btn" onClick={changeAvatar}>Change picture</button>
            <Show when={user()?.img}>
              <button class="btn" onClick={() => avAct.run(async () => { await api("/me/avatar", { method: "DELETE" }); await refreshUser(); })}>Remove</button>
            </Show>
          </div>
          <avAct.Note />
        </div>
      </section>
      <form class="panel form" onSubmit={savePw}>
        <h2>Password</h2>
        <label>Current password<input type="password" value={cur()} onInput={(e) => setCur(e.currentTarget.value)} autocomplete="current-password" /></label>
        <label>New password<input type="password" value={pw()} onInput={(e) => setPw(e.currentTarget.value)} autocomplete="new-password" /></label>
        <label>Confirm new password<input type="password" value={pw2()} onInput={(e) => setPw2(e.currentTarget.value)} autocomplete="new-password" /></label>
        <button class="btn primary">Save password</button>
        <pwAct.Note />
      </form>
    </>
  );
}

// ---------- Dashboard ----------

function Dashboard() {
  const [info, { refetch: reInfo }] = createResource(() => api<Info>("/admin/info"));
  const [scan, { refetch: reScan }] = createResource(() => api<ScanStatus>("/admin/scan"));
  const act = useAction();
  const timer = setInterval(() => { reInfo(); if (scan()?.running) reScan(); }, 3000);
  const fast = setInterval(() => scan()?.running && reScan(), 1000);
  onCleanup(() => { clearInterval(timer); clearInterval(fast); });

  const startScan = () =>
    act.run(async () => {
      try {
        await api("/admin/scan", { method: "POST", body: JSON.stringify({}) });
      } catch (e) {
        if (!(e instanceof ApiError && e.status === 409)) throw e;
      }
      reScan();
    });
  const power = (what: "restart" | "shutdown") => {
    const extra = what === "shutdown" && info()?.in_docker ? "\n\nRunning in Docker: the restart policy will start it again. Use `docker compose stop` to keep it off." : "";
    if (!confirm(`${what === "restart" ? "Restart" : "Shut down"} the Kellyfin server?${extra}`)) return;
    act.run(() => api(`/admin/${what}`, { method: "POST" }), what === "restart" ? "Restarting… reload in a few seconds." : "Shutting down…");
  };
  const exportJson = () =>
    act.run(async () => {
      const m = await api<unknown>("/admin/export");
      const a = document.createElement("a");
      a.href = URL.createObjectURL(new Blob([JSON.stringify(m, null, 2)], { type: "application/json" }));
      a.download = `kellyfin-export-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(a.href);
    });
  const importJson = async () => {
    const f = await pickFile("application/json,.json");
    if (!f) return;
    act.run(async () => {
      await api("/admin/import", { method: "POST", body: await f.text() });
      reScan();
    }, "Imported. A scan is running to pick up the files.");
  };

  return (
    <>
      <section class="stats">
        <div><b>{info()?.libraries ?? "–"}</b><span>Libraries</span></div>
        <div><b>{info()?.items ?? "–"}</b><span>Videos</span></div>
        <div><b>{info()?.users ?? "–"}</b><span>Users</span></div>
        <div><b>{info() ? (info()!.hw_device ? "VAAPI" : "CPU") : "–"}</b><span>Transcoding</span></div>
      </section>
      <section class="panel">
        <h2>Server</h2>
        <p class="muted">Kellyfin {info()?.version}</p>
        <div class="row-btns">
          <button class="btn primary" onClick={startScan} disabled={scan()?.running}>Scan All Libraries</button>
          <button class="btn" onClick={() => power("restart")}>Restart</button>
          <button class="btn danger" onClick={() => power("shutdown")}>Shutdown</button>
        </div>
        <Show when={scan()}>
          {(s) => (
            <p class="muted">
              <Show when={s().running} fallback={s().finished_at ? `Last scan finished ${new Date(s().finished_at! * 1000).toLocaleString()}` : "No scan since the server started"}>
                Scanning — {s().phase} {s().done}/{s().total}
              </Show>
            </p>
          )}
        </Show>
        <Show when={scan()?.error}><p class="error">{scan()!.error}</p></Show>
        <act.Note />
      </section>
      <section class="panel">
        <h2>Now playing</h2>
        <For each={info()?.sessions} fallback={<p class="muted">Nobody is watching.</p>}>
          {(s) => {
            const job = () => info()?.transcodes.find((t) => t.item_id === s.item_id);
            return (
              <div class="session">
                <A href={`/item/${s.item_id}`}><strong>{s.title}</strong></A>
                <span class="muted">
                  {s.user} · {fmtClock(s.position)}{s.duration ? ` / ${fmtClock(s.duration)}` : ""} · {s.mode || "?"}
                  {job() ? ` · ${job()!.p}p ${job()!.hw ? "VAAPI" : "x264"}${job()!.paused ? " (throttled)" : ""}` : ""}
                </span>
              </div>
            );
          }}
        </For>
      </section>
      <section class="panel">
        <h2>Import / export</h2>
        <p class="muted">Libraries, folders, titles and watch progress as JSON. Images are kept on the server (use <code>kellyfin export DIR</code> to include them).</p>
        <div class="row-btns">
          <button class="btn" onClick={exportJson}>Export JSON</button>
          <button class="btn" onClick={importJson}>Import JSON</button>
        </div>
      </section>
    </>
  );
}

// ---------- Users ----------

function Users() {
  const [users, { refetch }] = createResource(() => api<UserRow[]>("/admin/users"));
  const [name, setName] = createSignal("");
  const [pw, setPw] = createSignal("");
  const [admin, setAdmin] = createSignal(false);
  const act = useAction();
  const add = (e: Event) => {
    e.preventDefault();
    act.run(async () => {
      await api("/admin/users", { method: "POST", body: JSON.stringify({ name: name(), password: pw(), is_admin: admin() }) });
      setName(""); setPw(""); setAdmin(false);
      refetch();
    }, "User added");
  };
  const patch = (u: UserRow, body: object, done?: string) =>
    act.run(async () => { await api(`/admin/users/${u.id}`, { method: "PATCH", body: JSON.stringify(body) }); refetch(); }, done);
  return (
    <>
      <section class="panel">
        <h2>Users</h2>
        <table class="table">
          <tbody>
            <For each={users()}>
              {(u) => (
                <tr>
                  <td>
                    <strong>{u.name}</strong>
                    <Show when={!u.has_password}><span class="tag warn">no password</span></Show>
                  </td>
                  <td>
                    <label class="check">
                      <input type="checkbox" checked={u.is_admin} disabled={u.id === user()?.id} onChange={(e) => patch(u, { is_admin: e.currentTarget.checked })} />
                      Admin
                    </label>
                  </td>
                  <td class="row-btns end">
                    <button class="btn small" onClick={() => {
                      const p = prompt(`New password for ${u.name}`);
                      if (p) patch(u, { password: p }, `Password set for ${u.name}`);
                    }}>Set password</button>
                    <Show when={u.id !== user()?.id}>
                      <button class="btn small danger" onClick={() => {
                        if (confirm(`Delete ${u.name} and their watch history?`))
                          act.run(async () => { await api(`/admin/users/${u.id}`, { method: "DELETE" }); refetch(); });
                      }}>Delete</button>
                    </Show>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
        <act.Note />
      </section>
      <form class="panel form" onSubmit={add}>
        <h2>Add user</h2>
        <label>Name<input value={name()} onInput={(e) => setName(e.currentTarget.value)} autocapitalize="off" /></label>
        <label>Password<input type="password" value={pw()} onInput={(e) => setPw(e.currentTarget.value)} autocomplete="new-password" /></label>
        <label class="check"><input type="checkbox" checked={admin()} onChange={(e) => setAdmin(e.currentTarget.checked)} /> Administrator</label>
        <button class="btn primary">Add user</button>
      </form>
    </>
  );
}

// ---------- Libraries ----------

function LibraryEditor(props: { lib?: LibRow; onDone: () => void }) {
  const [name, setName] = createSignal(props.lib?.name ?? "");
  const [paths, setPaths] = createSignal<string[]>(props.lib?.paths ?? []);
  const [picking, setPicking] = createSignal(false);
  const act = useAction();
  const save = (e: Event) => {
    e.preventDefault();
    act.run(async () => {
      const body = JSON.stringify({ name: name(), paths: paths() });
      if (props.lib) await api(`/admin/libraries/${props.lib.id}`, { method: "PATCH", body });
      else await api("/admin/libraries", { method: "POST", body });
      props.onDone();
    });
  };
  return (
    <form class="panel form" onSubmit={save}>
      <h2>{props.lib ? `Edit ${props.lib.name}` : "Add library"}</h2>
      <label>Name<input value={name()} onInput={(e) => setName(e.currentTarget.value)} /></label>
      <div class="paths">
        <span class="muted">Folders</span>
        <For each={paths()} fallback={<p class="muted">No folder yet.</p>}>
          {(p) => (
            <div class="path-row">
              <code>{p}</code>
              <button type="button" class="btn small" onClick={() => setPaths(paths().filter((x) => x !== p))}>Remove</button>
            </div>
          )}
        </For>
        <button type="button" class="btn small" onClick={() => setPicking(true)}>+ Add folder</button>
      </div>
      <div class="row-btns">
        <button class="btn primary">{props.lib ? "Save" : "Add and scan"}</button>
        <button type="button" class="btn" onClick={props.onDone}>Cancel</button>
      </div>
      <act.Note />
      <Show when={picking()}>
        <FolderPicker
          start={paths()[paths().length - 1]?.replace(/\/[^/]*$/, "") || "/"}
          onClose={() => setPicking(false)}
          onPick={(p) => { setPaths([...new Set([...paths(), p])]); setPicking(false); }}
        />
      </Show>
    </form>
  );
}

function Libraries() {
  const [libs, { refetch: reRows }] = createResource(() => api<LibRow[]>("/admin/libraries"));
  const [cards, { refetch: reCards }] = createResource(() => api<Lib[]>("/libraries"));
  const refetch = () => { reRows(); reCards(); };
  const card = (id: number) => cards()?.find((c) => c.id === id);
  const [editing, setEditing] = createSignal<LibRow | "new" | null>(null);
  const [filter, setFilter] = createSignal("");
  const act = useAction();
  const shown = () => {
    const f = filter().toLowerCase();
    return (libs() ?? []).filter((l) => !f || l.name.toLowerCase().includes(f) || l.paths.some((p) => p.toLowerCase().includes(f)));
  };
  const done = () => { setEditing(null); refetch(); };
  const setImage = async (l: LibRow) => {
    const f = await pickFile("image/*");
    if (f) act.run(async () => { await upload(`/admin/libraries/${l.id}/image`, f); refetch(); }, `Image set for ${l.name}`);
  };
  return (
    <Switch>
      <Match when={editing() === "new"}><LibraryEditor onDone={done} /></Match>
      <Match when={editing()}>{(l) => <LibraryEditor lib={l() as LibRow} onDone={done} />}</Match>
      <Match when={true}>
        <section class="panel">
          <div class="panel-head">
            <h2>Libraries</h2>
            <input type="search" placeholder="Filter" value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} />
            <button class="btn primary small" onClick={() => setEditing("new")}>+ Add library</button>
          </div>
          <act.Note />
          <div class="lib-list">
            <For each={shown()}>
              {(l) => (
                <div class="lib-row">
                  <div class="lib-thumb" style={{ "background-color": card(l.id)?.color ?? undefined }}>
                    <Show when={card(l.id) && img(card(l.id)!, "s")}>{(src) => <img src={src()} alt="" loading="lazy" />}</Show>
                  </div>
                  <div class="lib-info">
                    <A href={`/library/${l.id}`}><strong>{l.name}</strong></A> <span class="muted">· {l.count} videos</span>
                    <For each={l.paths}>{(p) => <div class="muted small"><code>{p}</code></div>}</For>
                  </div>
                  <div class="row-btns end">
                    <button class="btn small" onClick={() => act.run(() => api("/admin/scan", { method: "POST", body: JSON.stringify({ library_id: l.id }) }), `Scanning ${l.name}`)}>Scan</button>
                    <button class="btn small" onClick={() => setEditing(l)}>Edit</button>
                    <button class="btn small" onClick={() => setImage(l)}>Image</button>
                    <Show when={l.has_image}>
                      <button class="btn small" title="Use the first video's thumbnail" onClick={() => act.run(async () => { await api(`/admin/libraries/${l.id}/image`, { method: "DELETE" }); refetch(); })}>Auto image</button>
                    </Show>
                    <button class="btn small danger" onClick={() => {
                      if (confirm(`Remove library "${l.name}" from Kellyfin? Files on disk are not touched.`))
                        act.run(async () => { await api(`/admin/libraries/${l.id}`, { method: "DELETE" }); refetch(); });
                    }}>Remove</button>
                  </div>
                </div>
              )}
            </For>
          </div>
        </section>
      </Match>
    </Switch>
  );
}

// ---------- Playback ----------

function Playback() {
  const [settings, { refetch }] = createResource(() => api<Record<string, string>>("/admin/settings"));
  const [info] = createResource(() => api<Info>("/admin/info"));
  const act = useAction();
  const save = (e: Event) => {
    e.preventDefault();
    const f = new FormData(e.currentTarget as HTMLFormElement);
    act.run(async () => {
      await api("/admin/settings", { method: "PUT", body: JSON.stringify(Object.fromEntries(f)) });
      refetch();
    }, "Saved");
  };
  return (
    <form class="panel form" onSubmit={save}>
      <h2>Transcoding</h2>
      <p class="muted">
        Used when a video cannot play as-is (HEVC/AV1 on some devices) or a lower quality is picked.
        Currently: {info() ? (info()!.hw_device ? `VAAPI on ${info()!.hw_device}` : "software (x264)") : "…"}
      </p>
      <Show when={settings()}>
        <label>Hardware acceleration
          <select name="transcode.hwaccel" value={settings()!["transcode.hwaccel"] ?? "auto"}>
            <option value="auto">VAAPI when available (AMD / Intel GPU)</option>
            <option value="none">None — encode on the CPU</option>
          </select>
        </label>
        <label>VAAPI device<input name="transcode.device" value={settings()!["transcode.device"] ?? "/dev/dri/renderD128"} /></label>
      </Show>
      <button class="btn primary">Save</button>
      <act.Note />
    </form>
  );
}

const TABS = [
  { id: "profile", label: "Profile", admin: false, view: Profile },
  { id: "dashboard", label: "Dashboard", admin: true, view: Dashboard },
  { id: "users", label: "Users", admin: true, view: Users },
  { id: "libraries", label: "Libraries", admin: true, view: Libraries },
  { id: "playback", label: "Playback", admin: true, view: Playback },
];

export default function Settings() {
  const params = useParams();
  const tabs = () => TABS.filter((t) => !t.admin || user()?.is_admin);
  const tab = () => tabs().find((t) => t.id === params.tab) ?? tabs()[0];
  return (
    <div class="settings">
      <nav class="tabs">
        <For each={tabs()}>{(t) => <A href={`/settings/${t.id}`} classList={{ active: tab().id === t.id }}>{t.label}</A>}</For>
      </nav>
      <div class="tab-body">
        <Show when={tab()} keyed>{(t) => <t.view />}</Show>
      </div>
    </div>
  );
}
