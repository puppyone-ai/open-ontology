import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

export default defineConfig({
  resolve: {
    alias: {
      "@open-ontology/sdk-ts": fileURLToPath(new URL("../../../packages/sdk-ts/src/index.ts", import.meta.url)),
    },
  },
  test: {
    environment: "node",
    include: ["design/drive/**/*.test.ts"],
    fileParallelism: false,
  },
});
