import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";

export default defineConfig({
  plugins: [solidPlugin()],
  server: {
    port: 5173,
    proxy: {
      "/ws": {
        target: "ws://localhost:3001",
        ws: true,
        rewriteWsOrigin: true,
      },
    },
  },
  build: {
    target: "esnext",
  },
  test: {
    environment: "node",
    globals: true,
  },
});
