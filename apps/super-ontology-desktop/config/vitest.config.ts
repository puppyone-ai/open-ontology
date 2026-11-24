import fs from "node:fs";
import path from "node:path";
import { defineConfig } from "vitest/config";

const appRoot = path.resolve(import.meta.dirname, "..");
// Where the code under test lives. One place to change when the source layout moves; each can be overridden to point a run at another tree.
const at = (variable: string, fallback: string) => path.resolve(process.env[variable] ?? path.join(appRoot, fallback));

// The renderer suites run once the renderer sources exist (src/renderer/model); until then they are skipped, not failed.
const rendererPresent = fs.existsSync(at("SO_MODEL_DIR", "src/renderer/model"));

export default defineConfig({
  resolve: {
    alias: {
      "@so-main": at("SO_MAIN_DIR", "electron/main/super-ontology"),
      "@so-shared": at("SO_SHARED_DIR", "src/shared/super-ontology"),
      "@so-contract": at("SO_SHARED_DIR", "src/shared/super-ontology"),
      "@so-model": at("SO_MODEL_DIR", "src/renderer/model"),
    },
  },
  test: {
    environment: "node",
    include: ["tests/unit/**/*.test.{mjs,ts,tsx}", "tests/integration/**/*.test.{mjs,ts}"],
    exclude: rendererPresent ? ["**/node_modules/**"] : ["**/node_modules/**", "tests/unit/renderer/**"],
    passWithNoTests: false,
    testTimeout: 20_000,
  },
});
