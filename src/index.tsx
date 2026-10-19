import { render } from "solid-js/web";
import { Router, Route } from "@solidjs/router";
import "./styles.css";
import "./viewport";
import App from "./App";
import Home from "./pages/Home";
import Library from "./pages/Library";
import Item from "./pages/Item";
import Settings from "./pages/Settings";
import Player from "./pages/Player";
import Search from "./pages/Search";

render(
  () => (
    <Router root={App}>
      <Route path="/" component={Home} />
      <Route path="/library/:id" component={Library} />
      <Route path="/item/:id" component={Item} />
      <Route path="/settings/:tab?" component={Settings} />
      <Route path="/search" component={Search} />
      <Route path="/play/:id" component={Player} />
    </Router>
  ),
  document.getElementById("root")!,
);
