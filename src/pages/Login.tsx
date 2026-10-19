import { createSignal, Show } from "solid-js";
import { isTauri, login, server, setServer } from "../api";

export default function Login() {
  const [url, setUrl] = createSignal(server());
  const [name, setName] = createSignal("");
  const [pw, setPw] = createSignal("");
  const [err, setErr] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const submit = async (e: Event) => {
    e.preventDefault();
    setErr("");
    setBusy(true);
    try {
      if (isTauri) setServer(url());
      await login(name(), pw());
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div class="login">
      <form onSubmit={submit}>
        <img src="/favicon.svg" alt="" width="56" height="56" />
        <h1>Kellyfin</h1>
        <Show when={isTauri}>
          <label>Server<input value={url()} onInput={(e) => setUrl(e.currentTarget.value)} placeholder="http://192.168.1.10:8024" inputmode="url" /></label>
        </Show>
        <label>Username<input value={name()} onInput={(e) => setName(e.currentTarget.value)} autocomplete="username" autocapitalize="off" /></label>
        <label>Password<input type="password" value={pw()} onInput={(e) => setPw(e.currentTarget.value)} autocomplete="current-password" /></label>
        <Show when={err()}><p class="error">{err()}</p></Show>
        <button class="primary" disabled={busy()}>Sign in</button>
      </form>
    </div>
  );
}
