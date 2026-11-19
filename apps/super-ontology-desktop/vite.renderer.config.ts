import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const root = fileURLToPath(new URL("./src/renderer", import.meta.url));

// The renderer is built as static files the Electron window loads; `dev:renderer` serves them for a browser, with the development
// bridge (scripts/dev-bridge.mjs) standing in for the preload script.
export default defineConfig({
  root,
  base: "./",
  plugins: [react()],
  build: { outDir: fileURLToPath(new URL("./dist/renderer", import.meta.url)), emptyOutDir: true, target: "es2023" },
  server: { host: "127.0.0.1", port: 5273, strictPort: true, proxy: { "/bridge": { target: "http://127.0.0.1:5274", changeOrigin: false } } },
});
