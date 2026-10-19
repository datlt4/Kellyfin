import { Show } from "solid-js";
import { applyUpdate, dismissUpdate, updateAvailable, updateDismissed, updateProgress } from "../update";

export default function UpdateBanner() {
  return (
    <Show when={updateAvailable() && !updateDismissed()}>
      {(_) => {
        const u = updateAvailable()!;
        return (
          <div class="update-banner">
            <span>
              Kellyfin <b>{u.version}</b> is available.
              {u.action === "none" ? " Your Mac's re-sign job will install it." : ""}
            </span>
            <Show when={updateProgress() !== null} fallback={
              <>
                <Show when={u.action !== "none"}>
                  <button class="btn small primary" onClick={() => applyUpdate().catch((e) => alert(String(e)))}>
                    {u.action === "install" ? "Update and restart" : "Download"}
                  </button>
                </Show>
                <button class="btn small" onClick={() => dismissUpdate(true)}>Later</button>
              </>
            }>
              <span class="muted">Installing… {Math.round(updateProgress()! * 100)}%</span>
            </Show>
          </div>
        );
      }}
    </Show>
  );
}
