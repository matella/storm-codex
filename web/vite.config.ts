import { defineConfig } from "vitest/config";
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
  // `css.include` : les feuilles de web/src/cast/ sont traitées en test, pour que leurs imports `?raw`
  // (tests de règles CSS) reçoivent le vrai texte — les autres CSS restent vides en test
  test: { environment: "node", css: { include: [/[\/]cast[\/][\w-]+\.css/] } },
});
