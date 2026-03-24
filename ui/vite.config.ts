import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";

/** Same-origin `/api` and `/ws` need a backend; dev + preview both proxy here. */
const backendProxy = {
  "/ws": {
    target: "ws://127.0.0.1:13401",
    ws: true,
    rewriteWsOrigin: true,
  },
  "/api": {
    target: "http://127.0.0.1:13401",
    changeOrigin: true,
  },
} as const;

export default defineConfig({
  plugins: [solidPlugin()],
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    proxy: { ...backendProxy },
  },
  // `vite preview` does not inherit `server.proxy` — without this, `/api/*` returns 404.
  preview: {
    proxy: { ...backendProxy },
  },
  build: {
    target: "esnext",
  },
  test: {
    environment: "jsdom",
    globals: true,
    exclude: ["e2e/**", "node_modules/**"],
  },
});
