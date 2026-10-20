import { useLocation } from "@solidjs/router";
import { createResource, Match, Show, Switch, type ParentProps } from "solid-js";
import { restore, user } from "./api";
import TopBar from "./components/TopBar";
import UpdateBanner from "./components/UpdateBanner";
import { autoCheck } from "./update";
import Login from "./pages/Login";
import { pullToRefresh } from "./pull";

export default function App(props: ParentProps) {
  const [ready] = createResource(restore);
  const loc = useLocation();
  autoCheck();
  return (
    <Switch>
      <Match when={ready.loading}>
        <div class="splash" />
      </Match>
      <Match when={!user()}>
        <Login />
      </Match>
      <Match when={user()}>
        <Show when={!loc.pathname.startsWith("/play/")} fallback={props.children}>
          <TopBar />
          <UpdateBanner />
          <div class="pull" ref={pullToRefresh}>↻</div>
          <main>{props.children}</main>
        </Show>
      </Match>
    </Switch>
  );
}
