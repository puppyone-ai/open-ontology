import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { DOCS_API_VERSION, DOCS_ERROR_DETAIL_KEYS, DOCS_ERROR_RETRYABLE, DOCS_PERSISTENT_EVENT_TYPES } from "../../shared/super-ontology/docs-contract.generated.mjs";

const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

describe("generated contract", () => {
  it("matches contracts/ (no drift)", () => {
    const run = spawnSync(process.execPath, [path.join(appRoot, "scripts", "gen-contract.mjs"), "--check"], { encoding: "utf8" });
    expect(run.stderr).toBe("");
    expect(run.status).toBe(0);
  });
  it("detects a stale generated file", () => {
    const sandbox = fs.mkdtempSync(path.join(fs.realpathSync(process.env.TMPDIR ?? "/tmp"), "so-contract-"));
    try {
      fs.cpSync(appRoot, path.join(sandbox, "apps", "super-ontology-desktop"), { recursive: true, filter: (source) => !source.includes("node_modules") });
      fs.cpSync(path.join(appRoot, "..", "..", "contracts"), path.join(sandbox, "contracts"), { recursive: true });
      const target = path.join(sandbox, "apps", "super-ontology-desktop", "shared", "super-ontology", "docs-contract.generated.mjs");
      fs.appendFileSync(target, "// tampered\n");
      const run = spawnSync(process.execPath, [path.join(sandbox, "apps", "super-ontology-desktop", "scripts", "gen-contract.mjs"), "--check"], { encoding: "utf8" });
      expect(run.status).toBe(1);
      expect(run.stderr).toContain("out of date with contracts/");
    } finally {
      fs.rmSync(sandbox, { recursive: true, force: true });
    }
  });
  it("exposes the allow-lists the main process uses", () => {
    expect(DOCS_API_VERSION).toMatch(/^\d+\.\d+$/u);
    expect(Object.keys(DOCS_ERROR_DETAIL_KEYS)).toEqual(Object.keys(DOCS_ERROR_RETRYABLE));
    expect(DOCS_PERSISTENT_EVENT_TYPES.length).toBeGreaterThan(0);
  });
});
