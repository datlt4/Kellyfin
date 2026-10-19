import { render } from "solid-js/web";
import { Router, Route } from "@solidjs/router";
import "./styles.css";
import App from "./App";
import Home from "./pages/Home";
import Library from "./pages/Library";
import Item from "./pages/Item";
import Settings from "./pages/Settings";

render(
  () => (
    <Router root={App}>
      <Route path="/" component={Home} />
      <Route path="/library/:id" component={Library} />
      <Route path="/item/:id" component={Item} />
      <Route path="/settings" component={Settings} />
    </Router>
  ),
  document.getElementById("root")!,
);
