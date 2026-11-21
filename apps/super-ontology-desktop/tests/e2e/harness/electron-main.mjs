// The Electron entry of a T1 run. It does what the packaged app does (it imports the app's own main process) and changes one thing:
// the native open dialog answers from a queue file the scenario writes (`SO_T1_DIALOG_QUEUE`, one JSON array of absolute paths per
// dialog), because nothing can click a native dialog from a script. Everything else is production code.
import { app, dialog } from "electron";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

// Off-screen by default: the window is moved far outside every display and shown without activating the app, and the app stays out of the
// Dock and the app switcher, so a run never takes the focus of the person using the machine. Rendering is not throttled for it.
if (process.env.SO_T1_OFFSCREEN === "1") {
  app.commandLine.appendSwitch("disable-backgrounding-occluded-windows");
  app.commandLine.appendSwitch("disable-renderer-backgrounding");
  if (process.platform === "darwin") app.setActivationPolicy("accessory");
  app.on("browser-window-created", (_event, window) => {
    const park = () => window.setBounds({ x: -12_000, y: -12_000, width: 1280, height: 820 });
    window.show = () => { park(); window.showInactive(); park(); };
    window.focus = () => {};
    park();
  });
}
const queueFile = process.env.SO_T1_DIALOG_QUEUE;
if (queueFile) {
  dialog.showOpenDialog = async () => {
    let queue = [];
    try { queue = JSON.parse(fs.readFileSync(queueFile, "utf8")); } catch { /* no answer queued */ }
    const next = queue.shift();
    fs.writeFileSync(queueFile, JSON.stringify(queue));
    return next ? { canceled: false, filePaths: next } : { canceled: true, filePaths: [] };
  };
}
// SO_T1_OFFSCREEN=1: the window is never shown and the app gets no Dock icon, so a run does not take the focus from whoever is using the Mac.
// Everything is read over the DevTools protocol; a hidden window keeps running (the harness starts Electron with --disable-renderer-backgrounding).
if (process.env.SO_T1_OFFSCREEN === "1") {
  const { BrowserWindow } = await import("electron");
  BrowserWindow.prototype.show = function show() {};
  BrowserWindow.prototype.focus = function focus() {};
  app.dock?.hide();
}
const appRoot = process.env.SO_APP_ROOT;
if (!appRoot || !path.isAbsolute(appRoot)) { console.error("SO_APP_ROOT must be the app directory"); app.exit(2); } else {
  await import(pathToFileURL(path.join(appRoot, "electron", "main", "index.mjs")).href);
}
