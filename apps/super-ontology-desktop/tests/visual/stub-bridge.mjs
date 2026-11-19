// A deterministic stand-in for the preload bridge, installed before the page loads: the built renderer shows fixed data, so its screens can be
// compared pixel for pixel. Answers the channels the shown screens read; every other channel answers an empty page.
export function stubBridgeSource(scenario) {
  return `(${function install(scenario) {
    const now = "2026-11-10T10:00:00Z";
    const snapshot = (status) => ({ revision: 1, status, reason: null, code: null, preflight: null, health: status === "healthy" ? { events_head: 0 } : null, restarts: { count: 0, nextAttemptAt: null }, ownedByDesktop: true, keepEngineOnQuit: false, startedAt: now, connection: { state: "connected", attempt: 0, nextRetryAt: null, lastSeq: 0 }, mode: "local", remote: null });
    const drive = (id, name, files, extra = {}) => ({ id, name, backend: "fs", config: { root: "/Users/example/" + name }, status: "active", revision: 1, counts: { published: files }, files_total: files, bytes_total: files * 2048, pending_review: 0, created_at: now, freshness: { state: "up_to_date" }, ...extra });
    const entity = (n, name, type, facts) => ({ id: "00000000-0000-4000-8000-0000000001" + String(n).padStart(2, "0"), key: name.toLowerCase().replace(/ /g, "-"), type, name, counts: { facts } });
    const drives = scenario.engine === "starting" ? [] : [drive("00000000-0000-4000-8000-000000000001", "My files", 3), drive("00000000-0000-4000-8000-000000000002", "Team docs", 12)];
    const entities = [entity(1, "Dana Lee", "Person", 4), entity(2, "Project Alpha", "Project", 5), entity(3, "Harbor feed", "Topic", 2), entity(4, "Projects/alpha.md", "Document", 3)];
    const empty = { items: [], next_cursor: null };
    const answer = (channel) => {
      switch (channel) {
        case "engine-read": case "engine-preflight": case "engine-start": case "engine-restart": return snapshot(scenario.engine);
        case "drives-list": return drives;
        case "entities-search": return { items: entities, next_cursor: null };
        case "ontology-versions": return { items: [{ n: 3, facts_added: 6, facts_retracted: 0, published_at: "2026-11-09T10:00:00Z" }], next_cursor: null };
        case "agent-token-list": return empty;
        case "db-sources-list": return { items: scenario.databases ? [{ name: "orders-db", kind: "postgres", host: "db.internal", port: 5432, database: "demo", user: "reader", include: ["public.orders"], tls: true }] : [] };
        case "model-config-read": return { configured: false };
        case "events-subscribe": return { instanceId: "stub", seq: 0 };
        default: return empty;
      }
    };
    const frozen = Date.parse("2026-11-12T10:00:00Z");
    window.localStorage.setItem("so.firstRun.done", scenario.firstRun ? "0" : "1");
    if (scenario.firstRun) window.localStorage.removeItem("so.firstRun.done");
    window.superOntology = {
      invoke: async (channel) => ({ ok: true, value: answer(channel) }),
      subscribe: () => () => undefined,
      pathsForFiles: () => ({ paths: [], unreadable: 0 }),
      openExternal: async () => undefined,
      shell: { info: async () => ({ name: "Super Ontology", version: "0.1.0", platform: "darwin", arch: "arm64", locale: "en-US", packaged: true, license: "Apache-2.0", releaseUrl: "" }), readSettings: async () => ({ language: "en", ooExecutable: null, runtime: "auto", keepEngineOnQuit: false, remote: null }), writeSettings: async (patch) => patch, onSettingsChanged: () => () => undefined, onMenuCommand: () => () => undefined, readNotice: async () => "", copyToClipboard: async () => true, openReleasePage: async () => true },
    };
    void frozen;
  }})(${JSON.stringify(scenario)})`;
}
