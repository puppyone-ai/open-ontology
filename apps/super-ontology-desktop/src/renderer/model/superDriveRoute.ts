import type { Drive } from "../../shared/super-ontology/docs-api.generated";

export type SuperDriveSection = "files" | "review" | "suggestions" | "shares" | "engine" | "settings";
export type DetailsTab = "overview" | "blocks" | "entities" | "shares" | "speakers";
export type ViewMode = "list" | "grid";
export type FileSort = "path" | "state_since";

/** Where the details pane should land inside a file: a block, a `[src:...]` label or a time in a recording. `nonce` makes an equal focus fire again. */
export type DetailsFocus = { blockId?: string; src?: string; seconds?: number; nonce: number };

export type SuperDriveRoute = {
  section: SuperDriveSection;
  driveId: string | null;
  /** Drive-relative folder, `""` for the root (no leading or trailing slash). */
  path: string;
  detailsFileId: string | null;
  detailsTab: DetailsTab;
  /** An entity opened on its own (search, review row, chip); takes the details column instead of a file. */
  entityKey: string | null;
  detailsFocus: DetailsFocus | null;
  /** A Share to bring into view in the share list. */
  focusShareId: string | null;
  view: ViewMode;
};

/** Filters of a folder listing. The default filter (list sorted by path, no filters) hashes to `""`. */
export type FolderFilter = {
  states?: readonly string[];
  q?: string;
  recursive?: boolean;
  sort?: FileSort;
};

export const INITIAL_ROUTE: SuperDriveRoute = Object.freeze({
  section: "files", driveId: null, path: "", detailsFileId: null, detailsTab: "overview", entityKey: null, detailsFocus: null, focusShareId: null, view: "list",
});

const SEPARATOR = "\u0000";

export function isDefaultFilter(filter: FolderFilter | undefined): boolean {
  if (!filter) return true;
  return (filter.states?.length ?? 0) === 0 && !filter.q && !filter.recursive && (filter.sort ?? "path") === "path";
}

/** Stable text for a filter; equal filters hash equal regardless of state order. */
export function hashFilter(filter: FolderFilter | undefined): string {
  if (isDefaultFilter(filter)) return "";
  const states = [...(filter?.states ?? [])].sort().join(",");
  return JSON.stringify([states, filter?.q ?? "", filter?.recursive === true, filter?.sort ?? "path"]);
}

export type FolderKey = string;

export function folderKey(driveId: string, path: string, filter?: FolderFilter): FolderKey {
  return `${driveId}${SEPARATOR}${path}${SEPARATOR}${hashFilter(filter)}`;
}

export function parseFolderKey(key: FolderKey): { driveId: string; path: string; filterHash: string } {
  const [driveId = "", path = "", filterHash = ""] = key.split(SEPARATOR);
  return { driveId, path, filterHash };
}

export function parentPath(path: string): string {
  const index = path.lastIndexOf("/");
  return index === -1 ? "" : path.slice(0, index);
}

export function baseName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

export function joinPath(folder: string, name: string): string {
  return folder === "" ? name : `${folder}/${name}`;
}

export type BreadcrumbSegment = { name: string; path: string };

/** Every ancestor of `path` from the first folder down to `path` itself (the Drive root is added by the caller). */
export function pathSegments(path: string): BreadcrumbSegment[] {
  if (path === "") return [];
  const names = path.split("/");
  return names.map((name, index) => ({ name, path: names.slice(0, index + 1).join("/") }));
}

export function navigateToFolder(route: SuperDriveRoute, path: string): SuperDriveRoute {
  return { ...route, section: "files", path, detailsFileId: null };
}

export function selectDriveRoute(route: SuperDriveRoute, driveId: string | null): SuperDriveRoute {
  return { ...route, driveId, path: "", detailsFileId: null };
}

export function driveForRoute(route: SuperDriveRoute, drives: Record<string, Drive>, order: readonly string[]): Drive | null {
  if (route.driveId && drives[route.driveId]) return drives[route.driveId];
  const first = order[0];
  return first ? drives[first] ?? null : null;
}
