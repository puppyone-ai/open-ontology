import { readFile } from "node:fs/promises";
import vm from "node:vm";
import { describe, expect, it, vi } from "vitest";
import {
  SUPER_ONTOLOGY_CHANNELS,
  SUPER_ONTOLOGY_PUSH_CHANNELS,
} from "@so-shared/requests.mjs";

const source = await readFile(new URL("../../../electron/preload.cjs", import.meta.url), "utf8");

function runPreload(ipcRenderer = {}) {
  const exposed = {};
  const context = {
    require: (specifier) => {
      if (specifier !== "electron") throw new Error(`Unexpected preload import: ${specifier}`);
      return {
        contextBridge: { exposeInMainWorld: (name, value) => { exposed[name] = value; } },
        ipcRenderer: {
          invoke: () => Promise.resolve(),
          on: () => undefined,
          removeListener: () => undefined,
          send: () => undefined,
          ...ipcRenderer,
        },
        webUtils: { getPathForFile: () => "" },
      };
    },
    console, Promise, Error, Set, Array,
  };
  vm.runInNewContext(source, context, { filename: "preload.cjs" });
  return exposed;
}

describe("Super Ontology preload bridge", () => {
  it("exposes exactly window.superOntology with invoke, subscribe, pathsForFiles, openExternal and shell", () => {
    const bridge = runPreload();
    expect(Object.keys(bridge)).toEqual(["superOntology"]);
    expect(bridge.superOntology).toEqual({
      invoke: expect.any(Function), subscribe: expect.any(Function), pathsForFiles: expect.any(Function), openExternal: expect.any(Function),
      shell: {
        info: expect.any(Function), readSettings: expect.any(Function), writeSettings: expect.any(Function), onSettingsChanged: expect.any(Function),
        onMenuCommand: expect.any(Function), readNotice: expect.any(Function), copyToClipboard: expect.any(Function), openReleasePage: expect.any(Function),
      },
    });
  });

  it("forwards shell calls to their own channels and refuses an unknown notice kind", async () => {
    const invoke = vi.fn(() => Promise.resolve("x"));
    const { shell } = runPreload({ invoke }).superOntology;
    await shell.readNotice("notice");
    expect(invoke).toHaveBeenCalledWith("shell:read-notice", "notice");
    expect(await shell.readNotice("passwd")).toBe("");
    await shell.copyToClipboard(5);
    expect(invoke).toHaveBeenLastCalledWith("shell:copy-text", "5");
  });

  it("forwards only whitelisted channels under the super-ontology: prefix", async () => {
    const invoke = vi.fn(() => Promise.resolve({ ok: true }));
    const bridge = runPreload({ invoke }).superOntology;
    await bridge.invoke("engine-read", {});
    expect(invoke).toHaveBeenCalledWith("super-ontology:engine-read", {});
    const rejected = await bridge.invoke("shares-delete-all", {});
    expect(rejected).toEqual({ ok: false, error: { code: "SO_INVALID_REQUEST", retryable: false, details: {} } });
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("subscribes to every push channel and unsubscribes them all", () => {
    const on = vi.fn();
    const removeListener = vi.fn();
    const unsubscribe = runPreload({ on, removeListener })
      .superOntology.subscribe(() => undefined);
    expect(on.mock.calls.map(([channel]) => channel).filter((name) => name.startsWith("super-ontology:"))).toEqual(
      SUPER_ONTOLOGY_PUSH_CHANNELS.map((name) => `super-ontology:${name}`),
    );
    unsubscribe();
    expect(removeListener.mock.calls.filter(([name]) => name.startsWith("super-ontology:"))).toHaveLength(
      SUPER_ONTOLOGY_PUSH_CHANNELS.length,
    );
  });

  it("keeps the preload channel lists equal to the shared contract", () => {
    const requestChannels = /const CHANNELS = new Set\((\[[^\]]*\])\)/u.exec(source);
    const pushChannels = /const PUSH_CHANNELS = (\[[^\]]*\])/u.exec(source);
    // The preload lists are array literals (trailing commas allowed), not JSON.
    expect(vm.runInNewContext(requestChannels[1]).sort()).toEqual([...SUPER_ONTOLOGY_CHANNELS].sort());
    expect(vm.runInNewContext(pushChannels[1])).toEqual([...SUPER_ONTOLOGY_PUSH_CHANNELS]);
  });
});
