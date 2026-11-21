import fs from "node:fs";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { createIgnoreStack, createPathIgnoreResolver, decideTree, ignoreReason, loadDirRules, parseRules } from "@so-main/ignore-rules.mjs";

// The Rust fs walk is tested against the same file (crates/oo-connectors/src/objectstore/path/ignore.rs): one set of decisions for copy,
// link and watch.
const fixture = JSON.parse(fs.readFileSync(path.resolve(import.meta.dirname, "../../../../../crates/oo-connectors/tests/fixtures/ignore-semantics.json"), "utf8"));

describe("ignore rules shared with the engine", () => {
  const cases = fixture.cases.flatMap((entry) => entry.paths.map((row) => [`${entry.name}: ${row.path}${row.dir ? "/" : ""}`, entry.files, row]));
  it("covers a broad set of decisions", () => { expect(cases.length).toBeGreaterThanOrEqual(100); });
  it.each(cases)("%s", (_label, files, row) => {
    expect(decideTree(files, row.path, row.dir === true)).toBe(row.ignored);
  });

  it("applies the same default skip table as the engine", () => {
    expect(fixture.defaults.length).toBeGreaterThanOrEqual(50);
    for (const row of fixture.defaults) expect(ignoreReason(row.name, row.dir) !== null, `${row.name} (dir: ${row.dir})`).toBe(row.skipped);
  });

  it("names the reason of a default skip", () => {
    expect(ignoreReason(".git", true)).toBe("hidden");
    expect(ignoreReason("node_modules", true)).toBe("junk");
    expect(ignoreReason("target", true)).toBe("junk");
    expect(ignoreReason("target", false)).toBeNull();
    expect(ignoreReason("a.pyc")).toBe("junk");
    expect(ignoreReason("~$a.docx")).toBe("temporary");
  });

  it("refuses hostile rules instead of getting slow", () => {
    expect(parseRules("a/**/b/**/c/**/d/**/e/**/f/z\n", "gitignore")).toEqual([]);
    expect(parseRules(`${"a/".repeat(40)}x\n`, "gitignore")).toEqual([]);
    const rules = parseRules("**/a/**/a/**/a/**/a/z\n", "ooignore");
    expect(rules).toHaveLength(1);
    const started = Date.now();
    expect(createIgnoreStack().with(0, rules).ignored(Array.from({ length: 60 }, () => "a"), false)).toBeNull();
    expect(Date.now() - started).toBeLessThan(2000);
  });
});

describe("rules files on disk", () => {
  it("reads .gitignore then .ooignore, skips oversized and non-regular files, and the watcher resolver follows edits", async () => {
    const root = await fsp.mkdtemp(path.join(os.tmpdir(), "so-ignore-"));
    try {
      await fsp.mkdir(path.join(root, "src"), { recursive: true });
      await fsp.writeFile(path.join(root, ".gitignore"), "*.md\n");
      await fsp.writeFile(path.join(root, ".ooignore"), "!keep.md\nsecret/\n");
      expect((await loadDirRules(root)).map((rule) => rule.source)).toEqual(["gitignore", "ooignore", "ooignore"]);
      await fsp.writeFile(path.join(root, "src", ".gitignore"), "x".repeat(1024 * 1024 + 1));
      expect(await loadDirRules(path.join(root, "src"))).toEqual([]);
      await fsp.rm(path.join(root, "src", ".gitignore"));
      await fsp.symlink(path.join(root, ".gitignore"), path.join(root, "src", ".gitignore"));
      expect(await loadDirRules(path.join(root, "src"))).toEqual([]);

      const resolver = createPathIgnoreResolver(root);
      expect(resolver.isIgnored("a.md")).toBe(true);
      expect(resolver.isIgnored("keep.md")).toBe(false);
      expect(resolver.isIgnored("secret/x.txt")).toBe(true);
      expect(resolver.isIgnored("node_modules/x/y.js")).toBe(true);
      expect(resolver.isIgnored("pkg/build/out.js")).toBe(true);
      expect(resolver.isIgnored("pkg/build")).toBe(false);
      expect(resolver.isIgnored("src/main.ts")).toBe(false);
      await fsp.writeFile(path.join(root, ".ooignore"), "*.ts\n");
      expect(resolver.isIgnored("src/main.ts")).toBe(false); // cached until the watcher says a rules file changed
      resolver.invalidate();
      expect(resolver.isIgnored("src/main.ts")).toBe(true);
    } finally { await fsp.rm(root, { recursive: true, force: true }); }
  });
});
