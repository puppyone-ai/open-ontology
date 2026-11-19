import { describe, expect, it, vi } from "vitest";
import { createIpcRegistry } from "../../../electron/main/ipc-registry.mjs";
import { registerSuperOntologyIpcHandlers } from "../../../electron/main/ipc-super-ontology.mjs";
import { createSuperOntologyService } from "@so-main/service.mjs";
import { SUPER_ONTOLOGY_CHANNELS } from "@so-shared/requests.mjs";

function createFakeIpcMain() {
  const handlers = new Map();
  return { handlers, ipcMain: { handle: (channel, listener) => { handlers.set(channel, listener); }, on: vi.fn() } };
}

const trustedEvent = () => ({ trusted: true, sender: { id: 1 } });

function setup({ available = true, service = createSuperOntologyService({ logger: { debug() {} } }) } = {}) {
  const { handlers, ipcMain } = createFakeIpcMain();
  const registry = createIpcRegistry({ ipcMain, isTrustedSender: (event) => event?.trusted === true, logger: { warn() {} } });
  const registered = registerSuperOntologyIpcHandlers({ ipcMain: registry.facade, service, available });
  return { handlers, registered, service };
}

describe("Super Ontology IPC handlers", () => {
  it("registers no channel when the capability is off", () => {
    const { handlers, registered } = setup({ available: false });
    expect(registered).toEqual([]);
    expect([...handlers.keys()].filter((name) => name.startsWith("super-ontology:"))).toEqual([]);
  });

  it("registers no channel without a service", () => {
    const { handlers, registered } = setup({ service: null });
    expect(registered).toEqual([]);
    expect(handlers.size).toBe(0);
  });

  it("registers exactly the implemented channels when the capability is on", () => {
    const { handlers, registered } = setup();
    expect(registered).toEqual([...SUPER_ONTOLOGY_CHANNELS]);
    expect([...handlers.keys()]).toEqual(SUPER_ONTOLOGY_CHANNELS.map((name) => `super-ontology:${name}`));
  });

  it("answers engine-read with the fixed idle snapshot in the result envelope", async () => {
    const { handlers } = setup();
    const result = await handlers.get("super-ontology:engine-read")(trustedEvent(), {});
    expect(result.ok).toBe(true);
    expect(result.value).toMatchObject({ status: "idle", connection: { state: "offline" } });
    expect(Object.keys(JSON.parse(JSON.stringify(result.value))).sort()).toEqual([
      "code", "connection", "health", "keepEngineOnQuit", "mode", "ownedByDesktop", "preflight",
      "reason", "remote", "restarts", "revision", "startedAt", "status",
    ]);
  });

  it("returns a fresh snapshot on each call so a caller cannot mutate shared state", async () => {
    const { handlers } = setup();
    const first = await handlers.get("super-ontology:engine-read")(trustedEvent(), {});
    first.value.status = "healthy";
    const second = await handlers.get("super-ontology:engine-read")(trustedEvent(), undefined);
    expect(second.value.status).toBe("idle");
  });

  it("hands the calling WebContents to the service so grants bind to the window", async () => {
    const seen = [];
    const service = { handlers: { "pick-path": async (request, context) => { seen.push(context.sender); return { cancelled: true }; } } };
    const { handlers } = setup({ service });
    const event = trustedEvent();
    await handlers.get("super-ontology:pick-path")(event, { purpose: "drive-root" });
    expect(seen).toEqual([event.sender]);
  });

  it("rejects an invalid request with SO_INVALID_REQUEST", async () => {
    const { handlers } = setup();
    const result = await handlers.get("super-ontology:engine-read")(trustedEvent(), { path: "/etc/passwd" });
    expect(result).toEqual({ ok: false, error: { code: "SO_INVALID_REQUEST", retryable: false, details: {} } });
  });

  it("never leaks a native error thrown by the service", async () => {
    const service = { handlers: { "engine-read": async () => { throw new Error("Bearer oodt_leaked"); } } };
    const { handlers } = setup({ service });
    const result = await handlers.get("super-ontology:engine-read")(trustedEvent(), {});
    expect(result.ok).toBe(false);
    expect(result.error.code).toBe("SO_INTERNAL");
    expect(JSON.stringify(result)).not.toContain("oodt_");
  });

  it("refuses a call from a sender that is not one of the app's own frames", async () => {
    const { handlers } = setup();
    const result = await handlers.get("super-ontology:engine-read")({ trusted: false, sender: {} }, {});
    expect(result).toEqual({ ok: false, error: { code: "SO_INVALID_REQUEST", retryable: false, details: {} } });
  });

  it("shuts down idempotently without touching anything", async () => {
    const { service } = setup();
    await service.shutdown();
    await expect(service.shutdown()).resolves.toBeUndefined();
  });
});
