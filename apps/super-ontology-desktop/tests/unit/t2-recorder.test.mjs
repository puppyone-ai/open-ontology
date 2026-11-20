import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { liveRecorders, startRecording, stopAllRecordings } from "../e2e/harness/recorder.mjs";
import { withLimit } from "../e2e/t2/run-packaged.mjs";

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
