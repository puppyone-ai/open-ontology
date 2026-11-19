import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// Serves tests/visual/gallery: the design-system components in fixed states, for the Playwright screenshot suite. Never part of the app build.
export default defineConfig({
  root: fileURLToPath(new URL("../tests/visual/gallery", import.meta.url)),
  plugins: [react()],
  server: { host: "127.0.0.1", port: 5275, strictPort: true, fs: { allow: [fileURLToPath(new URL("../../..", import.meta.url))] } },
});
