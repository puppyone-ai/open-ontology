import type { Drive } from "../../shared/super-ontology/docs-api.generated";
import { driveForRoute, folderKey, type FolderFilter } from "./superDriveRoute";
import { createEmptyFolderPage, type FileRecord, type FolderPage, type SuperDriveState } from "./superDriveState";

export const LIVE_STATUSES: readonly string[] = ["healthy", "degraded"];

export const selectEngine = (state: SuperDriveState) => state.engine;
export const selectEngineStatus = (state: SuperDriveState) => state.engine?.status ?? "idle";
export const selectIsEngineLive = (state: SuperDriveState) => LIVE_STATUSES.includes(selectEngineStatus(state));

export const selectDrives = (state: SuperDriveState): Drive[] => state.drives.order.map((id) => state.drives.byId[id]).filter(Boolean);
export const selectActiveDrive = (state: SuperDriveState): Drive | null => driveForRoute(state.route, state.drives.byId, state.drives.order);
export const selectHasNoDrives = (state: SuperDriveState) => state.drives.loaded && state.drives.order.length === 0;

const EMPTY_PAGE: FolderPage = Object.freeze(createEmptyFolderPage()) as FolderPage;

export function selectFolder(state: SuperDriveState, driveId: string | null, path: string, filter?: FolderFilter): FolderPage {
  if (!driveId) return EMPTY_PAGE;
  return state.folders[folderKey(driveId, path, filter)] ?? EMPTY_PAGE;
}

/** Records of a folder page, in page order; an id whose record is gone (removed) is skipped. */
export function selectFolderFiles(state: SuperDriveState, page: FolderPage): FileRecord[] {
  const rows: FileRecord[] = [];
  for (const id of page.ids) {
    const record = state.files[id];
    if (record) rows.push(record);
  }
  return rows;
}

export function selectPendingReviewTotal(state: SuperDriveState): number {
  let total = 0;
  for (const driveId of state.drives.order) {
    const counts = state.review.pendingByDrive[driveId];
    total += counts ? Object.values(counts).reduce((sum, value) => sum + value, 0) : state.drives.byId[driveId]?.pending_review ?? 0;
  }
  return total;
}

export const selectDriftedShareCount = (state: SuperDriveState): number => (
  state.shares.order.filter((id) => state.shares.byId[id]?.state === "drifted").length
);

/** Notices that are not tied to a control and fade on their own (reconnect catch-up, batch results). */
export const selectNotices = (state: SuperDriveState) => state.notices;
export const selectConnectionLabel = (state: SuperDriveState) => (state.connection.resyncing ? "resyncing" : state.connection.state);

/** The newest ontology version any Drive has published, from `ontology.version_published`: screens that read the ontology load again when it moves. */
export const selectOntologyVersion = (state: SuperDriveState): number => Object.values(state.ontology).reduce((newest, known) => Math.max(newest, known.n), 0);
