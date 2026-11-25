// Sandboxed preload: it cannot import shared modules, so the channel lists below repeat src/shared/super-ontology/requests.mjs
// (a unit test keeps them equal). The only global is `window.superOntology` (see PRELOAD.md).
const { contextBridge, ipcRenderer, webUtils } = require("electron");

const CHANNELS = new Set([
  "engine-read", "engine-preflight", "engine-start", "engine-stop", "engine-restart", "engine-set-keep",
  "engine-reveal-logs", "pick-path", "grant-dropped-folder", "engine-set-executable",
  "drives-list", "drive-get", "drive-create", "my-files-ensure", "drive-update", "drive-delete", "drive-sync", "drive-tree", "files-list",
  "events-subscribe", "events-since", "events-unsubscribe", "upload-start", "upload-cancel", "upload-discard", "upload-decide",
  "files-retry", "files-retry-matching", "file-detail", "file-blocks", "file-preview-url",
  "suggestions-list", "suggestions-digest", "suggestion-diff", "suggestion-decide", "suggestions-decide-source",
  "entities-search", "entity-detail", "resolve", "review-list", "review-decide", "pack-compile", "pack-read", "shares-list",
  "share-get", "share-mount", "share-refresh", "share-revoke", "share-reveal", "share-updates", "share-group-refresh",
  "usage-read", "ontology-versions", "model-config-read", "model-config-save", "fact-correct", "entity-merge", "entity-split", "constraints-list",
  "media-decode", "media-decode-cancel", "atlas-links", "ontology-schema", "profile-overrides-list", "profile-override-set", "structure-read", "structure-induce", "structure-decide", "structure-yaml", "structure-instances",
  "cloud-mode-read", "cloud-mode-set", "audit-shares", "audit-verify", "cloud-token-create", "cloud-token-revoke", "engine-set-runtime",
  "engine-connect-remote", "engine-disconnect-remote", "engine-state-read", "engine-data-use",
  "agent-token-create", "agent-token-list", "agent-token-revoke", "agent-connect-snippet",
  "ask-run", "ask-cancel",
  "cli-db-kinds", "cli-db-list", "cli-db-check", "cli-db-add", "cli-db-remove", "cli-db-test",
  "cli-doctor", "cli-pipeline", "cli-job-start", "cli-job-read", "cli-review-read", "cli-review-submit",
  "cli-gateway-read", "cli-gateway-set", "cli-gateway-clear", "cli-query",
  "cli-diagnostics", "cli-ontology-export",
]);
const PUSH_CHANNELS = ["engine-changed", "events", "connection-changed", "resync-required", "upload-progress", "ask-progress"];
const NOTICE_KINDS = new Set(["notice", "third-party", "license"]);
const failure = (code) => ({ ok: false, error: { code, retryable: false, details: {} } });

function listen(channel, callback) {
  if (typeof callback !== "function") return () => {};
  const listener = (_event, payload) => callback(payload);
  ipcRenderer.on(channel, listener);
  return () => ipcRenderer.removeListener(channel, listener);
}

contextBridge.exposeInMainWorld("superOntology", {
  pathsForFiles: (files) => {
    const paths = [];
    let unreadable = 0;
    for (const file of Array.from(files ?? [])) {
      let resolved = "";
      try { resolved = webUtils.getPathForFile(file); } catch { resolved = ""; }
      if (resolved) paths.push(resolved); else unreadable += 1;
    }
    return { paths, unreadable };
  },
  invoke: (channel, request) => (
    CHANNELS.has(channel) ? ipcRenderer.invoke(`super-ontology:${channel}`, request) : Promise.resolve(failure("SO_INVALID_REQUEST"))
  ),
  subscribe: (callback) => {
    if (typeof callback !== "function") return () => {};
    const listeners = PUSH_CHANNELS.map((channel) => {
      const listener = (_event, payload) => callback({ channel, payload });
      ipcRenderer.on(`super-ontology:${channel}`, listener);
      return [channel, listener];
    });
    return () => { for (const [channel, listener] of listeners) ipcRenderer.removeListener(`super-ontology:${channel}`, listener); };
  },
  openExternal: (url) => ipcRenderer.invoke("shell:open-external", String(url)),
  shell: {
    info: () => ipcRenderer.invoke("shell:info"),
    readSettings: () => ipcRenderer.invoke("shell:read-settings"),
    writeSettings: (patch) => ipcRenderer.invoke("shell:write-settings", patch),
    onSettingsChanged: (callback) => listen("shell:settings-changed", callback),
    onMenuCommand: (callback) => listen("shell:menu-command", callback),
    readNotice: (kind) => (NOTICE_KINDS.has(kind) ? ipcRenderer.invoke("shell:read-notice", kind) : Promise.resolve("")),
    /** Clipboard only; main refuses text over 16 KB or a call while the window is not focused (a click implies focus). */
    copyToClipboard: (text) => ipcRenderer.invoke("shell:copy-text", String(text)),
    openReleasePage: () => ipcRenderer.invoke("shell:open-release"),
  },
});
