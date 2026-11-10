import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Build → web/dist (served by storm-codex-server). In dev, API/WS proxy to the server :8088.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": "http://127.0.0.1:8088",
      "/ws": { target: "ws://127.0.0.1:8088", ws: true },
    },
  },
  build: { outDir: "dist", emptyOutDir: true },
  // `css.include`: the web/src/cast/ stylesheets are processed in tests, so that their `?raw` imports
  // (CSS rule tests) get the real text — other CSS stays empty in tests
  test: { environment: "node", css: { include: [/[\/]cast[\/][\w-]+\.css/] } },
});
