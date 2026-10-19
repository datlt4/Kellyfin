import { createSignal, For, Show } from "solid-js";
import {
  activeServer, isTauri, login, probeServer, restore, savedServers, saveServers, useServer, type ServerProfile,
} from "../api";

/** Add / edit a saved server: URL plus optional headers (e.g. a Pangolin access token). */
function ServerEditor(props: { server?: ServerProfile; onDone: (s: ServerProfile | null) => void }) {
  const [name, setName] = createSignal(props.server?.name ?? "");
  const [url, setUrl] = createSignal(props.server?.url ?? "");
  const [headers, setHeaders] = createSignal<[string, string][]>(props.server?.headers ?? []);
  const [note, setNote] = createSignal<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = createSignal(false);

  const current = (): ServerProfile => ({
    id: props.server?.id ?? crypto.randomUUID?.() ?? String(Date.now()),
    name: name().trim() || url().replace(/^https?:\/\//, ""),
    url: url().trim(),
    headers: headers().filter(([k]) => k.trim()),
  });
  const setHeader = (i: number, j: 0 | 1, v: string) =>
    setHeaders(headers().map((h, k) => (k === i ? ((j === 0 ? [v, h[1]] : [h[0], v]) as [string, string]) : h)));

  const test = async () => {
    setBusy(true);
    setNote(null);
    try {
      setNote({ ok: true, text: `Connected — Kellyfin ${await probeServer(current())}` });
    } catch (e) {
      setNote({ ok: false, text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  };
  const saveIt = (e: Event) => {
    e.preventDefault();
    if (!/^https?:\/\/.+/.test(url().trim())) return setNote({ ok: false, text: "URL must start with http:// or https://" });
    const s = current();
    saveServers([...savedServers().filter((x) => x.id !== s.id), s]);
    props.onDone(s);
  };
  const remove = () => {
    if (!props.server || !confirm(`Remove ${props.server.name}?`)) return;
    saveServers(savedServers().filter((x) => x.id !== props.server!.id));
    props.onDone(null);
  };

  return (
    <form class="server-form" onSubmit={saveIt}>
      <h2>{props.server ? "Edit server" : "Add server"}</h2>
      <label>Name<input value={name()} onInput={(e) => setName(e.currentTarget.value)} placeholder="Home" /></label>
      <label>URL<input value={url()} onInput={(e) => setUrl(e.currentTarget.value)} placeholder="http://192.168.1.15:8024 or https://kellyfin.example.com" inputmode="url" autocapitalize="off" /></label>
      <div class="headers">
        <span>Headers <small class="muted">(sent with every request — for Pangolin, Cloudflare Access…)</small></span>
        <For each={headers()}>
          {(h, i) => (
            <div class="header-row">
              <input value={h[0]} onInput={(e) => setHeader(i(), 0, e.currentTarget.value)} placeholder="Name" autocapitalize="off" />
              <input value={h[1]} onInput={(e) => setHeader(i(), 1, e.currentTarget.value)} placeholder="Value" autocapitalize="off" />
              <button type="button" class="btn small" onClick={() => setHeaders(headers().filter((_, k) => k !== i()))} aria-label="Remove header">✕</button>
            </div>
          )}
        </For>
        <button type="button" class="btn small" onClick={() => setHeaders([...headers(), ["", ""]])}>+ Add header</button>
      </div>
      <Show when={note()}>{(n) => <p class={n().ok ? "ok" : "error"}>{n().text}</p>}</Show>
      <div class="row-btns">
        <button type="button" class="btn" onClick={test} disabled={busy() || !url()}>Test</button>
        <button class="btn primary">Save</button>
        <Show when={savedServers().length}><button type="button" class="btn" onClick={() => props.onDone(props.server ?? null)}>Cancel</button></Show>
        <Show when={props.server}><button type="button" class="btn danger" onClick={remove}>Remove</button></Show>
      </div>
    </form>
  );
}

export default function Login() {
  const [servers, setServers] = createSignal(savedServers());
  // Apps with no saved server start in the editor.
  const [editing, setEditing] = createSignal<ServerProfile | "new" | null>(isTauri && !savedServers().length ? "new" : null);
  const [name, setName] = createSignal("");
  const [pw, setPw] = createSignal("");
  const [err, setErr] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  const pick = async (id: string) => {
    const s = servers().find((x) => x.id === id);
    if (!s) return;
    setErr("");
    await useServer(s).catch((e) => setErr(String(e)));
    await restore(); // signs straight in when this server still has a session
  };

  const submit = async (e: Event) => {
    e.preventDefault();
    setErr("");
    setBusy(true);
    try {
      await login(name(), pw());
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="login">
      <Show
        when={!editing()}
        fallback={
          <ServerEditor
            server={editing() === "new" ? undefined : (editing() as ServerProfile)}
            onDone={async (s) => {
              setServers(savedServers());
              setEditing(null);
              if (s) await pick(s.id);
            }}
          />
        }
      >
        <form onSubmit={submit}>
          <img src="/favicon.svg" alt="" width="56" height="56" />
          <h1>Kellyfin</h1>
          <Show when={isTauri}>
            <label>
              Server
              <div class="server-pick">
                <select value={activeServer()?.id ?? ""} onChange={(e) => pick(e.currentTarget.value)}>
                  <Show when={!activeServer()}><option value="">Choose a server</option></Show>
                  <For each={servers()}>{(s) => <option value={s.id}>{s.name}{s.headers.length ? " 🔒" : ""}</option>}</For>
                </select>
                <Show when={activeServer()}><button type="button" class="btn small" onClick={() => setEditing(activeServer())}>Edit</button></Show>
                <button type="button" class="btn small" onClick={() => setEditing("new")}>+ Add</button>
              </div>
            </label>
          </Show>
          <label>Username<input value={name()} onInput={(e) => setName(e.currentTarget.value)} autocomplete="username" autocapitalize="off" /></label>
          <label>Password<input type="password" value={pw()} onInput={(e) => setPw(e.currentTarget.value)} autocomplete="current-password" /></label>
          <Show when={err()}><p class="error">{err()}</p></Show>
          <button class="primary" disabled={busy() || (isTauri && !activeServer())}>Sign in</button>
        </form>
      </Show>
    </div>
  );
}
