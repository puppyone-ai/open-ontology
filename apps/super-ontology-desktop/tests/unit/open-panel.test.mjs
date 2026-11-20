import { describe, expect, it } from "vitest";
import { chooseThroughPanel } from "../e2e/harness/open-panel.mjs";

/** A panel that opens, takes keystrokes only while it is key, and closes on Open after the folder was entered and the files selected. */
function fakePanel({ missFirstGoTo = false, neverAccept = false, slowOpenMs = 0 } = {}) {
  const log = [];
  let clock = 0;
  let opened = slowOpenMs === 0;
  const panel = { open: slowOpenMs === 0, folder: false, selected: false, goTo: false, clipboard: "", missed: false, opens: 1 };
  const deps = {
    log,
    sheetCount: async () => (panel.open ? 1 : 0),
    press: async (chord) => {
      log.push(chord);
      if (chord === "cmd+shift+g") { if (missFirstGoTo && !panel.missed) { panel.missed = true; return; } panel.goTo = true; }
      if (chord === "cmd+v" && panel.goTo) panel.path = panel.clipboard;
      if (chord === "cmd+a" && panel.folder) panel.selected = true;
    },
    key: async (code) => {
      log.push(`key${code}`);
      if (code === 53) { panel.open = false; panel.folder = false; panel.selected = false; panel.goTo = false; }
      if (code === 36 && panel.goTo && panel.path && !panel.folder) { panel.folder = true; panel.goTo = false; return; }
      if (code === 36 && panel.folder && panel.selected && !neverAccept) panel.open = false;
    },
    setClipboard: async (text) => { panel.clipboard = text; },
    front: async () => { log.push("front"); },
    reopen: async () => { panel.open = true; panel.opens += 1; log.push("reopen"); },
    sleep: async (ms) => { clock += ms; if (!opened && clock >= slowOpenMs) { opened = true; panel.open = true; } },
    now: () => clock,
  };
  return { deps, panel, log };
}

describe("driving the native open panel", () => {
  it("succeeds in one attempt when the panel takes the keystrokes", async () => {
    const { deps, panel, log } = fakePanel();
    const result = await chooseThroughPanel("/tmp/Drop three", deps, { settleMs: 1 });
    expect(result.attempts).toBe(1);
    expect(panel.open).toBe(false);
    expect(log[0]).toBe("front");
  });

  it("waits for a panel that is still opening before sending any key", async () => {
    const { deps, log } = fakePanel({ slowOpenMs: 2000 });
    const result = await chooseThroughPanel("/tmp/Drop three", deps, { settleMs: 1 });
    expect(result.attempts).toBe(1);
    expect(log.indexOf("front")).toBeGreaterThanOrEqual(0);
    expect(log.indexOf("cmd+shift+g")).toBeGreaterThan(log.indexOf("front"));
  });

  it("a missed Go to folder leaves the panel open, so it is cancelled, reopened and tried again, and the attempts are reported", async () => {
    const { deps, panel, log } = fakePanel({ missFirstGoTo: true });
    const result = await chooseThroughPanel("/tmp/Drop three", deps, { settleMs: 1 });
    expect(result.attempts).toBe(2);
    expect(log).toContain("key53");
    expect(log).toContain("reopen");
    expect(panel.open).toBe(false);
  });

  it("fails loudly, with the panel closed, when no attempt gets through", async () => {
    const { deps, panel } = fakePanel({ neverAccept: true });
    await expect(chooseThroughPanel("/tmp/Drop three", deps, { settleMs: 1, attempts: 2 })).rejects.toThrow(/did not accept/u);
    expect(panel.open).toBe(false);
  });
});
