# Preload interface (`window.superOntology`)

Contract between the Electron main process (lane A) and the ported Super Drive renderer (lane B).
The preload is sandboxed (`contextIsolation: true`, `sandbox: true`, `nodeIntegration: false`) and exposes exactly one global, `window.superOntology`.
There is no feature flag and no renderer command-line argument: the bridge is always present in this app.

## Shape

```ts
interface SuperOntologyBridge {
  /** Host paths of dropped or picked File objects; a File without a host path is counted in `unreadable`. */
  pathsForFiles(files: ArrayLike<File>): { paths: string[]; unreadable: number };
  /** Request/response. Unknown channel => { ok:false, error:{ code:"SO_INVALID_REQUEST", retryable:false, details:{} } }. */
  invoke<C extends SuperOntologyChannel>(channel: C, request: ChannelRequest<C>): Promise<SuperOntologyResult<ChannelValue<C>>>;
  /** Push channels from main. Returns the unsubscribe function. */
  subscribe(callback: (push: { channel: PushChannel; payload: unknown }) => void): () => void;
  /** http(s) only; anything else is refused silently. Opens in the OS browser. */
  openExternal(url: string): Promise<void>;
  /** App shell, not the engine. */
  shell: {
    info(): Promise<ShellInfo>;
    readSettings(): Promise<ShellSettings>;
    writeSettings(patch: Partial<ShellSettings>): Promise<ShellSettings>;       // validated, unknown keys rejected
    onSettingsChanged(callback: (settings: ShellSettings) => void): () => void;
    readNotice(kind: "notice" | "third-party" | "license"): Promise<string>;    // About page text
    openReleasePage(): Promise<void>;                                            // v1 has no auto-update
  };
}
```

Renderer reader replacement: the reader returns `window.superOntology ?? null` (it was a nested property of another global in the source app);
`superDriveApi.openExternal` calls `window.superOntology?.openExternal?.(url)`.

## Channels (`invoke`)

Same names and request/response shapes as the earlier Super Drive bridge (`shared/super-ontology/requests.mjs`, `bridge.d.ts`), carried unchanged:

`engine-read engine-preflight engine-start engine-stop engine-restart engine-set-keep engine-reveal-logs pick-path engine-set-executable engine-set-runtime`
`drives-list drive-get drive-create drive-update drive-delete drive-sync drive-tree files-list`
`events-subscribe events-since events-unsubscribe upload-start upload-cancel files-retry files-retry-matching file-detail file-blocks file-preview-url`
`suggestions-list suggestions-digest suggestion-diff suggestion-decide suggestions-decide-source`
`entities-search entity-detail resolve review-list review-decide pack-compile pack-read shares-list share-get share-mount share-refresh share-revoke share-reveal share-updates share-group-refresh`
`usage-read ontology-versions model-config-read model-config-save fact-correct entity-merge entity-split constraints-list media-decode media-decode-cancel`
`cloud-mode-read cloud-mode-set audit-shares audit-verify cloud-token-create cloud-token-revoke`

Differences from the earlier bridge:

* `cloud-mode-read` always answers `{ mode:"local", configured:false, host:null, remoteUrl:null }` and `cloud-mode-set` with `"cloud"` fails `SO_CLOUD_NOT_CONFIGURED` (there is no hosted service in this app; remote engines use the two channels below).
* `pick-path` purposes: `drive-root | mount-target | oo-executable | token-file`.
* Added channels:
  * `engine-connect-remote` request `{ url: string; tokenGrantId: string; operationId?: string }` -> `EngineSnapshot`. `url` is `http(s)://host[:port]` of an `oo serve`; `tokenGrantId` is a `pick-path` grant with purpose `token-file` (file holds one `oodt_...` token, mode 0600). The engine is then not owned by the app (`ownedByDesktop:false`).
  * `engine-disconnect-remote` request `{ operationId?: string }` -> `EngineSnapshot`; back to local mode (no auto-start).
* `EngineSnapshot` gains `mode: "local" | "remote"` and `remote: { host: string } | null` (hosts only, never the token). `EnginePreflight.oo.source` is `"user" | "path" | "bundled" | null`.
* `EngineStatus` is unchanged; the Docker-related statuses stay valid (runtime `docker` is still supported when Docker runs; `auto` prefers the embedded `lite` runtime of `oo serve`).

## Push channels (`subscribe`)

`engine-changed events connection-changed resync-required upload-progress` (payloads unchanged).

## Shell types

```ts
interface ShellInfo {
  name: "Super Ontology"; version: string; platform: NodeJS.Platform; arch: string;
  locale: string;                 // OS locale, BCP-47
  packaged: boolean;
  license: "Apache-2.0";
  releaseUrl: string;
}
interface ShellSettings {
  language: "system" | "en" | "zh-Hans";
  theme: "system" | "light" | "dark";
  ooExecutable: string | null;    // absolute path chosen by the user; main-owned, writable only via engine-set-executable
  runtime: "auto" | "docker" | "lite";
  keepEngineOnQuit: boolean;
  remote: { url: string; tokenFile: string } | null;   // main-owned; writable only via engine-connect-remote
}
```

`writeSettings` accepts `language`, `theme` only; the other keys are read-only to the renderer and change through the engine channels above.

## Main-side ownership

* IPC channel names on the wire: `super-ontology:<channel>` (invoke), `super-ontology:<push>` (push), `shell:<name>`.
* Every handler is registered through `electron/main/ipc-registry.mjs`, which checks the sender is the app window's main frame loaded from the app's own origin (`app://renderer` in production, the Vite dev URL in development).
* Content-Security-Policy and navigation are fixed by main; the renderer cannot open new windows (`openExternal` only).
* Drive files are served on the `oo-drive:` scheme (`file-preview-url` values), registered by main.
