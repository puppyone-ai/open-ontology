import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { liveRecorders, startRecording, stopAllRecordings } from "../e2e/harness/recorder.mjs";
import { saveEngineLogs, withLimit } from "../e2e/t2/run-packaged.mjs";

const alive = (pid) => { try { process.kill(pid, 0); return true; } catch { return false; } };
const tmp = () => path.join(fs.mkdtempSync(path.join(os.tmpdir(), "rec-")), "run.mov");
const wait = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });
const sleeper = ["sleep", ["600"]];

describe("T2 recorder limits", () => {
  it("stop() ends the process and is idempotent", async () => {
    const r = startRecording(tmp(), { command: sleeper[0], args: sleeper[1] });
    expect(alive(r.pid)).toBe(true);
    await r.stop(); await r.stop();
    expect(alive(r.pid)).toBe(false);
    expect(liveRecorders()).toBe(0);
  });
  it("escalates to SIGKILL when the process ignores SIGINT", async () => {
    const r = startRecording(tmp(), { command: "sh", args: ["-c", "trap '' INT; while :; do sleep 1; done"], graceMs: 300 });
    await wait(200);
    await r.stop();
    expect(alive(r.pid)).toBe(false);
  });
  it("hard time cap stops a recorder nobody stops", async () => {
    const r = startRecording(tmp(), { command: sleeper[0], args: sleeper[1], maxMs: 200, graceMs: 200 });
    await wait(1200);
    expect(alive(r.pid)).toBe(false);
  });
  it("size cap stops a recorder whose file grows past it", async () => {
    const file = tmp();
    const r = startRecording(file, { command: sleeper[0], args: sleeper[1], maxBytes: 100, pollMs: 50, graceMs: 200 });
    fs.writeFileSync(file, Buffer.alloc(4096));
    await wait(1000);
    expect(alive(r.pid)).toBe(false);
  });
  it("a scenario timeout kills the recorder of the hung attempt", async () => {
    const r = startRecording(tmp(), { command: sleeper[0], args: sleeper[1] });
    const result = await withLimit(new Promise(() => undefined), 100, stopAllRecordings);
    expect(result.ok).toBe(false);
    await wait(300);
    expect(alive(r.pid)).toBe(false);
  });
});

describe("a failed attempt keeps the engine logs", () => {
  it("copies the logs of the profile and lists its files, skipping the database files", async () => {
    const profile = fs.mkdtempSync(path.join(os.tmpdir(), "so-profile-"));
    const destination = path.join(profile, "..", `${path.basename(profile)}-evidence`);
    fs.mkdirSync(path.join(profile, "logs"), { recursive: true });
    fs.mkdirSync(path.join(profile, "engine", "state", "pg", "data"), { recursive: true });
    fs.writeFileSync(path.join(profile, "logs", "super-ontology-serve.log"), "oo serve: downloading the pinned uv\n");
    fs.writeFileSync(path.join(profile, "engine", "state", "engine.log"), "engine log\n");
    fs.writeFileSync(path.join(profile, "engine", "state", "pg", "data", "postgresql.log"), "not copied\n");
    await saveEngineLogs(profile, destination);
    expect(fs.readFileSync(path.join(destination, "logs__super-ontology-serve.log"), "utf8")).toContain("downloading the pinned uv");
    expect(fs.existsSync(path.join(destination, "engine__state__engine.log"))).toBe(true);
    expect(fs.existsSync(path.join(destination, "engine__state__pg__data__postgresql.log"))).toBe(false);
    expect(fs.readFileSync(path.join(destination, "listing.txt"), "utf8")).toContain("engine/state/engine.log");
    fs.rmSync(profile, { recursive: true, force: true });
    fs.rmSync(destination, { recursive: true, force: true });
  });
});
