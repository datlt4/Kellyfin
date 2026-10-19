import { createResource, Match, Switch, type ParentProps } from "solid-js";
import { restore, user } from "./api";
import TopBar from "./components/TopBar";
import Login from "./pages/Login";

export default function App(props: ParentProps) {
  const [ready] = createResource(restore);
  return (
    <Switch>
      <Match when={ready.loading}>
        <div class="splash" />
      </Match>
      <Match when={!user()}>
        <Login />
      </Match>
      <Match when={user()}>
        <TopBar />
        <main>{props.children}</main>
      </Match>
    </Switch>
  );
}
