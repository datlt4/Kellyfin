import { useLocation } from "@solidjs/router";
import { createResource, Match, Show, Switch, type ParentProps } from "solid-js";
import { restore, user } from "./api";
import TopBar from "./components/TopBar";
import Login from "./pages/Login";

export default function App(props: ParentProps) {
  const [ready] = createResource(restore);
  const loc = useLocation();
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
          <main>{props.children}</main>
        </Show>
      </Match>
    </Switch>
  );
}
