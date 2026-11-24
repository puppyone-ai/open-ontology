// Development run: Vite dev server for src/renderer (when it exists) plus Electron pointed at it.
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const hasRenderer = fs.existsSync(path.join(root, "src", "renderer", "index.html"));
const children = [];
let env = { ...process.env };
if (hasRenderer) {
  const vite = spawn("pnpm", ["exec", "vite"], { cwd: root, stdio: "inherit" });
  children.push(vite);
  env = { ...env, SUPER_ONTOLOGY_DEV_URL: "http://127.0.0.1:5273" };
  await new Promise((resolve) => setTimeout(resolve, 1500));
}
const electron = spawn("pnpm", ["exec", "electron", "."], { cwd: root, stdio: "inherit", env });
children.push(electron);
electron.on("exit", (code) => { for (const child of children) child.kill("SIGTERM"); process.exit(code ?? 0); });
