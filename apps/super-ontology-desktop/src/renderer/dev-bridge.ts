import type { SuperOntologyBridge, SuperOntologyPush } from "../shared/super-ontology/bridge";

/**
 * Only used in `vite dev` (a plain browser has no preload script): speaks to scripts/dev-bridge.mjs, which runs the real main
 * process service. Never bundled into a production build.
 */
export function installDevBridge(): void {
  if (window.superOntology) return;
  const listeners = new Set<(push: SuperOntologyPush) => void>();
  let source: EventSource | null = null;
  const bridge: SuperOntologyBridge = {
    invoke: async (channel, request) => {
      const response = await fetch("/bridge/invoke", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ channel, request }) });
      return response.json();
    },
    subscribe: (callback) => {
      listeners.add(callback);
      if (!source) {
        source = new EventSource("/bridge/push");
        source.onmessage = (event) => { const push = JSON.parse(event.data) as SuperOntologyPush; for (const l of [...listeners]) l(push); };
      }
      return () => { listeners.delete(callback); if (listeners.size === 0) { source?.close(); source = null; } };
    },
    pathsForFiles: () => ({ paths: [], unreadable: 0 }),
    openExternal: async (url) => { window.open(url, "_blank", "noopener"); },
  };
  window.superOntology = bridge;
}
