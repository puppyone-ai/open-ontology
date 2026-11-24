import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const appRoot = path.resolve(import.meta.dirname, "..", "..");
const doc = fs.readFileSync(path.join(appRoot, "TEST-IDS.md"), "utf8");
const scenarioDirectory = path.join(appRoot, "tests", "e2e", "t1");
const scenarioText = fs.readdirSync(scenarioDirectory).filter((name) => name.endsWith(".mjs")).map((name) => fs.readFileSync(path.join(scenarioDirectory, name), "utf8")).join("\n");

// Table rows: the first cell lists ids in backticks, optionally followed by **(T1)** (the group is used by a scenario).
const documented = new Map();
for (const line of doc.split("\n")) {
  if (!line.startsWith("| `")) continue;
  const cell = line.split("|")[1];
  const marked = cell.includes("**(T1)**");
  for (const [, id] of cell.matchAll(/`([a-z][a-z0-9-]*)`/gu)) documented.set(id, marked);
}
const used = new Set([...scenarioText.matchAll(/\b(?:click|type|waitFor|exists|count|text|attr|attrs|dropFiles)\(\s*"([a-z][a-z0-9-]*)"/gu)].map((m) => m[1]));
for (const [, id] of scenarioText.matchAll(/data-testid=\\?"([a-z][a-z0-9-]*)\\?"/gu)) used.add(id);
for (const [, id] of scenarioText.matchAll(/`nav-\$\{|"(nav-[a-z]+)"/gu)) if (id) used.add(id);
used.add("nav-home"); used.add("nav-sources"); used.add("nav-ontology"); used.add("nav-agents"); used.add("nav-settings"); // openSeeded(screen)

describe("test ids", () => {
  it("every id a scenario uses is documented", () => {
    expect([...used].filter((id) => !documented.has(id)).sort()).toEqual([]);
  });
  it("every id marked as used by T1 is used by a scenario", () => {
    expect([...documented].filter(([id, marked]) => marked && !used.has(id)).map(([id]) => id).sort()).toEqual([]);
  });
  it("every id marked as used by T1 is in the renderer sources (once they exist)", () => {
    const sources = path.join(appRoot, "src", "renderer");
    if (!fs.existsSync(sources)) return;
    const files = [];
    const walk = (directory) => { for (const entry of fs.readdirSync(directory, { withFileTypes: true })) { const full = path.join(directory, entry.name); if (entry.isDirectory()) walk(full); else if (/\.(tsx?|jsx?|html)$/u.test(entry.name)) files.push(full); } };
    walk(sources);
    const text = files.map((file) => fs.readFileSync(file, "utf8")).join("\n");
    const missing = [...documented].filter(([id, marked]) => marked && !text.includes(`"${id}"`) && !(id.startsWith("nav-") && text.includes("nav-${")) && !text.includes(`'${id}'`)).map(([id]) => id);
    expect(missing.sort()).toEqual([]);
  });
});
