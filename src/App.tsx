import { useLocation } from "@solidjs/router";
import { createResource, Match, Show, Switch, type ParentProps } from "solid-js";
import { isTauri, restore } from "./api";
import TopBar from "./components/TopBar";
import UpdateBanner from "./components/UpdateBanner";
import Toast from "./components/Toast";
import { autoCheck } from "./update";
import { pullToRefresh } from "./pull";

// The desktop apps have no browser reload: F5 / Ctrl+R / Cmd+R do it.
if (isTauri)
  window.addEventListener("keydown", (e) => {
    if (e.key === "F5" || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "r")) {
      e.preventDefault();
      location.reload();
    }
  });

export default function App(props: ParentProps) {
  const [ready] = createResource(restore);
  const loc = useLocation();
  autoCheck();
  return (
    <Switch>
      <Match when={ready.loading}>
        <div class="splash" />
      </Match>
      <Match when={true}>
        <Show when={!loc.pathname.startsWith("/play/")} fallback={props.children}>
          <TopBar />
          <UpdateBanner />
          <Toast />
          <div class="pull" ref={pullToRefresh}>↻</div>
          <main>{props.children}</main>
        </Show>
      </Match>
    </Switch>
  );
}
