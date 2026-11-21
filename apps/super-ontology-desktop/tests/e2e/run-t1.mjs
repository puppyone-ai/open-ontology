#!/usr/bin/env node
// Runs the T1 scenarios one after another, each in its own process, at most 2 attempts and 5 minutes per attempt.
//   node tests/e2e/run-t1.mjs [--only first-run,home] [--engine real|fake]
// A scenario that exits 77 had nothing to run on (no oo binary): reported as skipped, and the run fails unless --allow-skip.
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const option = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
const only = option("--only")?.split(",");
const order = ["engine-boot", "first-run", "home", "sources", "databases", "ontology", "atlas", "query", "agents", "settings"];
// Needs labelled local database containers (see the scenario header); never part of a default run.
const optional = ["databases-live", "cli-live", "drop-large", "drop-latency"];
const selected = (only ? [...order, ...optional] : order).filter((name) => !only || only.includes(name));
const env = { ...process.env, ...(option("--engine") ? { SO_T1_ENGINE: option("--engine") } : {}) };
const ATTEMPT_MS = 5 * 60_000;
// The live database path starts containers, runs the real build and calls a real model.
const attemptMs = (name) => (["databases-live", "cli-live", "drop-large", "drop-latency"].includes(name) ? 30 * 60_000 : ATTEMPT_MS);

function attempt(name) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [path.join(here, "t1", `${name}.t1.mjs`)], { env, stdio: "inherit" });
    const timer = setTimeout(() => { console.error(`[t1] ${name} exceeded ${attemptMs(name) / 60_000} minutes`); child.kill("SIGKILL"); }, attemptMs(name));
    child.on("exit", (code) => { clearTimeout(timer); resolve(code ?? 1); });
  });
}

const results = [];
for (const name of selected) {
  let code = await attempt(name);
  let attempts = 1;
  if (code !== 0 && code !== 77) { console.error(`[t1] ${name} failed (exit ${code}); one more attempt`); code = await attempt(name); attempts = 2; }
  results.push({ name, code, attempts });
}
console.log("\n[t1] summary");
for (const { name, code, attempts } of results) console.log(`  ${code === 0 ? "PASS " : code === 77 ? "SKIP " : "FAIL "} ${name} (${attempts} attempt${attempts > 1 ? "s" : ""})`);
const failed = results.filter((entry) => entry.code !== 0 && entry.code !== 77);
const skipped = results.filter((entry) => entry.code === 77);
fs.writeSync(1, "");
process.exit(failed.length > 0 || (skipped.length > 0 && !args.includes("--allow-skip")) ? 1 : 0);
