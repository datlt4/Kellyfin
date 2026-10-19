import { createResource, createSignal, onCleanup, Show } from "solid-js";
import { api, ApiError, user, type ScanStatus } from "../api";

function ScanPanel() {
  const [status, { refetch }] = createResource(() => api<ScanStatus>("/admin/scan"));
  const [err, setErr] = createSignal("");
  const timer = setInterval(() => status()?.running && refetch(), 1000);
  onCleanup(() => clearInterval(timer));
  const start = async () => {
    setErr("");
    try {
      await api("/admin/scan", { method: "POST", body: JSON.stringify({}) });
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 409)) setErr(String(e));
    }
    refetch();
  };
  return (
    <section class="panel">
      <h2>Libraries</h2>
      <button class="primary" onClick={start} disabled={status()?.running}>Scan All Libraries</button>
      <Show when={status()}>
        {(s) => (
          <p class="muted">
            <Show when={s().running} fallback={s().finished_at ? `Last scan finished ${new Date(s().finished_at! * 1000).toLocaleString()}` : "No scan yet"}>
              Scanning: {s().phase} {s().done}/{s().total}
            </Show>
          </p>
        )}
      </Show>
      <Show when={err() || status()?.error}><p class="error">{err() || status()?.error}</p></Show>
    </section>
  );
}

export default function Settings() {
  return (
    <div class="settings">
      <h1>Settings</h1>
      <Show when={user()?.is_admin} fallback={<p class="muted">Profile settings arrive in P4.</p>}>
        <ScanPanel />
      </Show>
    </div>
  );
}
