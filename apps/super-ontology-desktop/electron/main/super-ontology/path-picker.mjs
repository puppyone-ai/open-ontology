import fs from "node:fs/promises";
import { constants as fsConstants } from "node:fs";

/** Per purpose: native dialog title key (locales/native) and dialog properties. */
const PICKER_OPTIONS = Object.freeze({
  "drive-root": { titleKey: "native.pick.driveRoot", properties: ["openDirectory"] },
  "mount-target": { titleKey: "native.pick.mountTarget", properties: ["openDirectory", "createDirectory"] },
  "oo-executable": { titleKey: "native.pick.ooExecutable", properties: ["openFile"] },
  "token-file": { titleKey: "native.pick.tokenFile", properties: ["openFile"] },
  "ca-file": { titleKey: "native.pick.caFile", properties: ["openFile"] },
  "export-dir": { titleKey: "native.pick.exportDir", properties: ["openDirectory", "createDirectory"] },
});

/**
 * Native path picker for the host-path purposes. Returns the canonical
 * (realpath) selection or `null` when cancelled. Only the main process ever
 * sees the dialog; the renderer gets a grant (see grants.mjs).
 */
export function createNativePathPicker({ dialog, translate, getOwnerWindow = () => null, fsPromises = fs } = {}) {
  return async function pickPath({ sender, purpose }) {
    const config = PICKER_OPTIONS[purpose];
    if (!config) return null;
    // macOS shows `message` above the file list and ignores `title`; the other systems use `title`.
    const heading = translate(config.titleKey);
    const options = { title: heading, message: heading, properties: [...config.properties] };
    const owner = getOwnerWindow(sender);
    const result = owner && !owner.isDestroyed?.()
      ? await dialog.showOpenDialog(owner, options)
      : await dialog.showOpenDialog(options);
    if (result.canceled || !Array.isArray(result.filePaths) || result.filePaths.length === 0) return null;
    const canonical = await fsPromises.realpath(result.filePaths[0]);
    const stat = await fsPromises.stat(canonical);
    if (purpose === "oo-executable" || purpose === "token-file" || purpose === "ca-file") {
      if (!stat.isFile()) return null;
      if (purpose === "oo-executable") await fsPromises.access(canonical, fsConstants.X_OK);
    } else if (!stat.isDirectory()) {
      return null;
    }
    return canonical;
  };
}
