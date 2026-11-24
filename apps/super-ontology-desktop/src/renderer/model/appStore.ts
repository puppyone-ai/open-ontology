import { superDriveApi } from "./superDriveApi";
import { createSuperDriveStore, type SuperDriveStore } from "./superDriveStore";

/** One store per window: every screen reads the same replica of the engine. */
let sharedStore: SuperDriveStore | null = null;
export function getStore(): SuperDriveStore {
  sharedStore ??= createSuperDriveStore({ api: superDriveApi });
  return sharedStore;
}
