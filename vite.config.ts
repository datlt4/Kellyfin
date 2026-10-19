import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import pkg from "./package.json";

const host = process.env.TAURI_DEV_HOST;
// Dev server proxies the API to a running kellyfin server.
const backend = process.env.KELLYFIN_DEV_SERVER ?? "http://localhost:8080";

export default defineConfig({
  plugins: [solid()],
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**", "**/server/**"] },
    proxy: { "/api": backend, "/img": backend },
  },
  build: { target: "es2020" },
});
