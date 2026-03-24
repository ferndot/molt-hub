import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";

export default defineConfig({
  plugins: [solidPlugin()],
  server: {
    port: 5173,
    proxy: {
      "/ws": {
        target: "ws://localhost:13401",
        ws: true,
        rewriteWsOrigin: true,
      },
      "/api": {
        target: "http://localhost:13401",
        changeOrigin: true,
      },
    },
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
