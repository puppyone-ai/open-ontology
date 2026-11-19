// The Electron entry of a T1 run. It does what the packaged app does (it imports the app's own main process) and changes one thing:
// the native open dialog answers from a queue file the scenario writes (`SO_T1_DIALOG_QUEUE`, one JSON array of absolute paths per
// dialog), because nothing can click a native dialog from a script. Everything else is production code.
import { app, dialog } from "electron";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

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
const appRoot = process.env.SO_APP_ROOT;
if (!appRoot || !path.isAbsolute(appRoot)) { console.error("SO_APP_ROOT must be the app directory"); app.exit(2); } else {
  await import(pathToFileURL(path.join(appRoot, "electron", "main", "index.mjs")).href);
}
