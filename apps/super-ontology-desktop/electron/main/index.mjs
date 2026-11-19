import { app, BrowserWindow, Menu, clipboard, dialog, ipcMain, protocol, session, shell } from "electron";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { APP_ORIGIN, APP_SCHEME, APP_SCHEME_PRIVILEGES, createAppProtocolHandler } from "./app-protocol.mjs";
import { createLegalReader, readReleaseUrl } from "./about.mjs";
import { createIpcRegistry } from "./ipc-registry.mjs";
import { registerSuperOntologyIpcHandlers } from "./ipc-super-ontology.mjs";
import { buildMenuTemplate } from "./menu.mjs";
import { bundledExecutablePath, createEnvironmentReader, createOoBootstrap } from "./oo-bootstrap.mjs";
import { RENDERER_WRITABLE, createSettingsStore } from "./settings-store.mjs";
import { createTranslator, resolveLanguage } from "./strings.mjs";
import { createMainWindow, createSenderCheck, isExternalUrl, originOf } from "./window.mjs";
import { DRIVE_FILE_SCHEME, DRIVE_FILE_SCHEME_PRIVILEGES, registerDriveFileProtocol } from "./super-ontology/drive-file-protocol.mjs";
import { createNativePathPicker } from "./super-ontology/path-picker.mjs";
import { createSuperOntologyService } from "./super-ontology/service.mjs";

const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const APP_NAME = "Super Ontology";
const devOrigin = !app.isPackaged && process.env.SUPER_ONTOLOGY_DEV_URL ? originOf(process.env.SUPER_ONTOLOGY_DEV_URL) : null;

app.setName(APP_NAME);
if (process.env.SUPER_ONTOLOGY_USER_DATA && path.isAbsolute(process.env.SUPER_ONTOLOGY_USER_DATA)) app.setPath("userData", process.env.SUPER_ONTOLOGY_USER_DATA);

protocol.registerSchemesAsPrivileged([
  { scheme: APP_SCHEME, privileges: APP_SCHEME_PRIVILEGES },
  { scheme: DRIVE_FILE_SCHEME, privileges: DRIVE_FILE_SCHEME_PRIVILEGES },
]);

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  void main();
}

async function main() {
  await app.whenReady();
  const userData = app.getPath("userData");
  const logger = { debug() {}, info() {}, warn: (message, meta) => console.warn(`[super-ontology] ${message}`, meta ?? ""), error: console.error };
  const settings = createSettingsStore({ file: path.join(userData, "settings.json"), logger });
  const initial = await settings.read();
  let language = resolveLanguage(initial.language, app.getLocale());
  settings.onChange((next) => { language = resolveLanguage(next.language, app.getLocale()); installMenu(); });
  const t = createTranslator(() => language);

  const windows = new Set();
  const getWindows = () => [...windows];
  const isTrustedSender = createSenderCheck({ getWindows, devOrigin });
  const registry = createIpcRegistry({ ipcMain, isTrustedSender, logger });
  const broadcast = (channel, payload) => {
    for (const win of windows) if (!win.isDestroyed()) win.webContents.send(`super-ontology:${channel}`, payload);
  };

  const engineRoot = path.join(userData, "engine");
  const resourcesPath = app.isPackaged ? process.resourcesPath : null;
  const legalDirectories = app.isPackaged ? [path.join(process.resourcesPath, "legal")] : [appRoot, path.join(appRoot, "build", "generated", "legal")];
  const generatedDirectories = app.isPackaged ? [path.join(process.resourcesPath, "legal")] : [path.join(appRoot, "build", "generated", "legal")];
  const pickPath = createNativePathPicker({
    dialog, translate: t, getOwnerWindow: (sender) => BrowserWindow.fromWebContents(sender) ?? null,
  });
  const service = createSuperOntologyService({
    logger,
    platformHost: { platform: "macos" },
    paths: { stateDir: path.join(engineRoot, "state"), logsDir: path.join(engineRoot, "logs"), managedFilesDir: path.join(userData, "My files") },
    settings,
    bundledExecutable: () => bundledExecutablePath({ resourcesPath, appRoot }),
    readEnvironment: createEnvironmentReader(),
    broadcast,
    pickPath,
    revealPath: (location) => shell.showItemInFolder(location),
  });
  const bootstrap = createOoBootstrap({ service });
  registerSuperOntologyIpcHandlers({ ipcMain: registry.facade, service });

  // Shell channels (settings, info, legal text, external links).
  const readNotice = createLegalReader({ directories: legalDirectories });
  const releaseUrl = () => readReleaseUrl({ directories: generatedDirectories });
  const openRelease = async () => { const url = await releaseUrl(); if (url) await shell.openExternal(url); return url !== null; };
  registry.handle("shell:info", async () => ({
    name: APP_NAME, version: app.getVersion(), platform: process.platform, arch: process.arch, locale: app.getLocale(),
    packaged: app.isPackaged, license: "Apache-2.0", releaseUrl: await releaseUrl(),
  }));
  registry.handle("shell:read-settings", () => settings.read());
  registry.handle("shell:write-settings", async (_event, patch) => {
    if (!patch || typeof patch !== "object" || Array.isArray(patch) || Object.keys(patch).some((key) => !RENDERER_WRITABLE.includes(key))) {
      throw new Error("SO_INVALID_REQUEST");
    }
    return settings.update(patch);
  });
  registry.handle("shell:read-notice", async (_event, kind) => readNotice(kind));
  registry.handle("shell:copy-text", async (event, text) => {
    const win = BrowserWindow.fromWebContents(event.sender);
    if (typeof text !== "string" || text.length === 0 || text.length > 16 * 1024 || !win?.isFocused()) throw new Error("SO_INVALID_REQUEST");
    clipboard.writeText(text);
    return true;
  });
  registry.handle("shell:open-release", () => openRelease());
  registry.handle("shell:open-external", async (_event, url) => { if (isExternalUrl(url)) await shell.openExternal(url); });
  settings.onChange((next) => { for (const win of windows) if (!win.isDestroyed()) win.webContents.send("shell:settings-changed", next); });

  // Protocols and session hardening.
  const rendererRoot = fs.existsSync(path.join(appRoot, "dist", "renderer", "index.html")) ? path.join(appRoot, "dist", "renderer") : path.join(appRoot, "resources", "bootstrap");
  protocol.handle(APP_SCHEME, createAppProtocolHandler({ root: rendererRoot }));
  registerDriveFileProtocol({ protocol, handler: service.driveFileHandler });
  session.defaultSession.setPermissionRequestHandler((_webContents, _permission, callback) => callback(false));
  session.defaultSession.setPermissionCheckHandler(() => false);

  const sendCommand = (command) => { const win = getWindows()[0]; if (win && !win.isDestroyed()) win.webContents.send("shell:menu-command", command); };
  function installMenu() {
    Menu.setApplicationMenu(Menu.buildFromTemplate(buildMenuTemplate({
      t, appName: APP_NAME, onCommand: sendCommand, openReleasePage: openRelease,
      revealLogs: () => { void service.handlers["engine-reveal-logs"]({ target: "logs" }).catch(() => {}); }, isDev: devOrigin !== null,
    })));
  }
  installMenu();
  app.setAboutPanelOptions({ applicationName: APP_NAME, applicationVersion: app.getVersion(), copyright: "Apache-2.0" });

  const open = () => {
    const win = createMainWindow({
      BrowserWindow, shell, devOrigin, rendererUrl: devOrigin ? `${devOrigin}/` : `${APP_ORIGIN}/index.html`,
      onClosed: () => { windows.delete(win); },
    });
    windows.add(win);
    return win;
  };
  open();
  void bootstrap.start();

  app.on("second-instance", () => {
    const win = getWindows()[0];
    if (!win) { open(); return; }
    if (win.isMinimized()) win.restore();
    win.focus();
  });
  app.on("activate", () => { if (windows.size === 0) open(); });
  app.on("window-all-closed", () => { app.quit(); });
  let quitting = false;
  app.on("before-quit", (event) => {
    if (quitting) return;
    event.preventDefault();
    quitting = true;
    const done = () => app.exit(0);
    // `shutdown` signals the engine first and then waits up to its grace period; a hung shutdown never blocks the quit for long.
    Promise.race([bootstrap.shutdown(), new Promise((resolve) => { setTimeout(resolve, 12_000).unref?.(); })]).then(done, done);
  });
}
