import path from "node:path";
import { fileURLToPath } from "node:url";
import { APP_ORIGIN } from "./app-protocol.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
/** `new URL(...).origin` is "null" for a custom scheme, so the origin is built from protocol and host. */
export const originOf = (value) => { const url = new URL(String(value)); return `${url.protocol}//${url.host}`; };

export const PRELOAD_PATH = path.join(here, "..", "preload.cjs");

/** `http(s)` only, no credentials: the only URLs the app hands to the OS browser. */
export function isExternalUrl(value) {
  try {
    const url = new URL(String(value));
    return (url.protocol === "https:" || url.protocol === "http:") && !url.username && !url.password;
  } catch { return false; }
}

/**
 * The single app window. `rendererUrl` is `app://renderer/index.html` (production) or a loopback dev server. Navigation away from
 * that origin and every `window.open` is refused; links go to the OS browser instead.
 */
export function createMainWindow({ BrowserWindow, shell, rendererUrl, devOrigin = null, backgroundColor = "#FAFAF9", onClosed = () => {} }) {
  const allowedOrigin = devOrigin ?? APP_ORIGIN;
  const win = new BrowserWindow({
    width: 1280, height: 820, minWidth: 960, minHeight: 640, show: false, backgroundColor, title: "Super Ontology",
    webPreferences: {
      preload: PRELOAD_PATH, contextIsolation: true, sandbox: true, nodeIntegration: false, webSecurity: true,
      spellcheck: false, devTools: devOrigin !== null,
    },
  });
  win.once("ready-to-show", () => win.show());
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (isExternalUrl(url)) void shell.openExternal(url);
    return { action: "deny" };
  });
  win.webContents.on("will-navigate", (event, url) => {
    let origin = null;
    try { origin = originOf(url); } catch { /* refused below */ }
    if (origin !== allowedOrigin) {
      event.preventDefault();
      if (isExternalUrl(url)) void shell.openExternal(url);
    }
  });
  win.on("closed", onClosed);
  void win.loadURL(rendererUrl);
  return win;
}

/** An IPC sender is trusted only when it is the main frame of an app window and still on the app's own origin. */
export function createSenderCheck({ getWindows, devOrigin = null }) {
  const allowedOrigin = devOrigin ?? APP_ORIGIN;
  return function isTrustedSender(event) {
    const frame = event?.senderFrame;
    if (!frame) return false;
    const windows = getWindows();
    if (!windows.some((win) => !win.isDestroyed() && win.webContents.mainFrame === frame)) return false;
    try { return originOf(frame.url) === allowedOrigin; } catch { return false; }
  };
}
