// Builds the renderer when it exists (lane B owns src/renderer). Without it the package carries the stand-in page of resources/bootstrap.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
if (!fs.existsSync(path.join(root, "src", "renderer", "index.html"))) {
  console.log("No src/renderer/index.html: skipping the renderer build (the stand-in page is used).");
  process.exit(0);
}
const result = spawnSync("pnpm", ["exec", "vite", "build"], { cwd: root, stdio: "inherit" });
process.exit(result.status ?? 1);
