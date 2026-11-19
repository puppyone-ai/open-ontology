import react from "@vitejs/plugin-react";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const root = path.dirname(fileURLToPath(import.meta.url));

// The renderer lives in src/renderer; its build goes to dist/renderer, which the app serves on app://renderer.
export default defineConfig({
  root: path.join(root, "src", "renderer"),
  base: "./",
  plugins: [react()],
  clearScreen: false,
  server: { host: "127.0.0.1", port: 5273, strictPort: true },
  build: { outDir: path.join(root, "dist", "renderer"), emptyOutDir: true, target: "chrome140", sourcemap: false },
});
