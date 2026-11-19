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
const optional = ["databases-live"];
const selected = (only ? [...order, ...optional] : order).filter((name) => !only || only.includes(name));
