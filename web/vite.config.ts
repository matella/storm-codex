import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Build → web/dist (servi par storm-codex-server). En dev, proxy API/WS vers le serveur :8088.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": "http://127.0.0.1:8088",
      "/ws": { target: "ws://127.0.0.1:8088", ws: true },
    },
  },
  build: { outDir: "dist", emptyOutDir: true },
});
