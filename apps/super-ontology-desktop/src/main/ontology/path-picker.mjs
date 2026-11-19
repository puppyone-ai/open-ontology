import fs from "node:fs/promises";
import { constants as fsConstants } from "node:fs";

/** Per purpose: native dialog title key (locales/native) and dialog properties. */
const PICKER_OPTIONS = Object.freeze({
  "drive-root": { titleKey: "native.superDrive.pickDriveRoot.title", properties: ["openDirectory"] },
  "mount-target": { titleKey: "native.superDrive.pickMountTarget.title", properties: ["openDirectory", "createDirectory"] },
  "oo-executable": { titleKey: "native.superDrive.pickOoExecutable.title", properties: ["openFile"] },
});

/**
 * Native path picker for the three host-path purposes. Returns the canonical
 * (realpath) selection or `null` when cancelled. Only the main process ever
 * sees the dialog; the renderer gets a grant (see grants.mjs).
 */
export function createNativePathPicker({ dialog, translate, getOwnerWindow = () => null, fsPromises = fs } = {}) {
  return async function pickPath({ sender, purpose }) {
    const config = PICKER_OPTIONS[purpose];
    if (!config) return null;
    const options = { title: translate(config.titleKey), properties: [...config.properties] };
    const owner = getOwnerWindow(sender);
    const result = owner && !owner.isDestroyed?.()
      ? await dialog.showOpenDialog(owner, options)
      : await dialog.showOpenDialog(options);
    if (result.canceled || !Array.isArray(result.filePaths) || result.filePaths.length === 0) return null;
    const canonical = await fsPromises.realpath(result.filePaths[0]);
    const stat = await fsPromises.stat(canonical);
    if (purpose === "oo-executable") {
      if (!stat.isFile()) return null;
      await fsPromises.access(canonical, fsConstants.X_OK);
    } else if (!stat.isDirectory()) {
      return null;
    }
    return canonical;
  };
}
