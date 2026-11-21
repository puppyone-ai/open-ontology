import { useEffect, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import type { EngineFileDetail, EngineFileSummary } from "../../shared/super-ontology/bridge";
import { invoke, type ApiResult } from "./bridgeInvoke";
import { pool } from "../features/atlas/adapter";

/**
 * "Not connected" (25 section 7.3): which files or things have nothing to do with the rest, and the plain reason why. A file is
 * unconnected when it produced no entities, could not be turned into entities (and the reason says which of unsupported, still
 * processing, waiting for review, too large, encrypted, parse failed), or when none of its entities is shared with another file
 * or source: connectivity is judged across files, never inside one. An entity is unconnected when the island it belongs to (the
 * entities joined by relations) touches one file only and no table row.
 * The reason is data; the sentence comes from the catalogs (`useUnlinkedText`).
 */
export type UnlinkedReason =
  | { kind: "no_entities" }
  | { kind: "unsupported"; format: string }
  | { kind: "processing" }
  | { kind: "review" }
  | { kind: "parse_failed"; code: string }
  | { kind: "too_large" }
  | { kind: "encrypted" }
  /** Entities found, but none of them appears in any other file or source. `count` is how many things the file holds. */
  | { kind: "no_relations"; count: number }
  /** An entity on an island that touches one file only (the atlas side); `file` names that file. */
  | { kind: "entity"; file?: string };

export type FileFacts = { entities: number; connected: boolean };
/** The fields of a file summary this module reads. */
export type FileLike = { id?: string; drive_id?: string; state: string; state_code?: unknown; path: string; format?: unknown; pending_review?: unknown };

const pending = (file: FileLike): boolean => typeof file.pending_review === "number" && file.pending_review > 0;
const PROCESSING = new Set(["stored", "queued", "parsing", "parsed", "extracting", "indexed"]);
const READY = new Set(["published", "extract_partial"]);
const IGNORED = new Set(["deleted", "superseded", "duplicate", "outdated"]);

/** The extension of a path in lower case, without the dot: the name of the format when the engine gives none. */
export const formatOf = (file: Pick<FileLike, "path" | "format">): string => {
  if (typeof file.format === "string" && file.format && file.format !== "unknown") return file.format;
  const name = file.path.slice(file.path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  return dot > 0 && dot < name.length - 1 ? name.slice(dot + 1).toLowerCase() : "?";
};

/**
 * Why a file is not connected, or `null` when it is (or when nothing can be said yet). `facts` is what the file produced; `null`
 * means it was not read, and a ready file is then left out rather than guessed.
 */
export function fileReason(file: FileLike, facts: FileFacts | null): UnlinkedReason | null {
  if (IGNORED.has(file.state)) return null;
  if (file.state === "unsupported") return { kind: "unsupported", format: formatOf(file) };
  if (file.state === "parse_failed") return { kind: "parse_failed", code: typeof file.state_code === "string" && file.state_code ? file.state_code : "PARSE_FAILED" };
  if (file.state === "too_large") return { kind: "too_large" };
  if (file.state === "encrypted") return { kind: "encrypted" };
  // An indexed file whose facts wait for a person is not still being processed: the engine finished and asks to be reviewed.
  if (file.state === "indexed" && pending(file)) return { kind: "review" };
  if (PROCESSING.has(file.state)) return { kind: "processing" };
  if (!READY.has(file.state)) return null;
  if (facts === null) return pending(file) ? { kind: "review" } : null;
  if (facts.entities === 0) return pending(file) ? { kind: "review" } : { kind: "no_entities" };
  return facts.connected ? null : { kind: "no_relations", count: facts.entities };
}

export type UnlinkedFile = { fileId: string; driveId: string; path: string; name: string; reason: UnlinkedReason };
export type UnlinkedEntity = { key: string; name: string; type: string; fileIds: string[]; /** Names of those files, for the side area. */ fileNames?: string[] };
export type UnlinkedReport = {
  files: UnlinkedFile[];
  entities: UnlinkedEntity[];
  /** Files that count (not deleted or superseded), files whose entities were read, and whether the reading was cut short. */
  totalFiles: number;
  checkedFiles: number;
  /** The ids of the files whose entities were read: a ready file in this set and not in `files` is connected. */
  read: string[];
  limited: boolean;
  /** Seen on every pass; a pass that failed to list anything leaves this false. */
  complete: boolean;
};

export type UnlinkedSource = {
  drives(): Promise<ApiResult<{ id: string; name: string; status?: string }[]>>;
  files(input: { driveId: string; cursor?: string }): Promise<ApiResult<{ items: EngineFileSummary[]; next_cursor?: string | null }>>;
  detail(fileId: string): Promise<ApiResult<EngineFileDetail>>;
  /** Files that hold an entity linked to a database row (the atlas draws these as links). */
  linkedFilePaths(): Promise<Set<string>>;
};

type EntityRefLike = { key?: string; name?: string; type?: string };
type FactLike = { status?: string; subject?: EntityRefLike; object?: EntityRefLike | null };
const detailParts = (detail: EngineFileDetail) => detail as EngineFileDetail & { entities?: EntityRefLike[]; facts?: FactLike[] };

export type AnalyzeOptions = { maxFiles?: number; concurrency?: number; signal?: { aborted: boolean } };

/**
 * Reads the Drives' files and, for each file that finished processing, the entities and facts it produced (one request per file,
 * a few at a time). `limited` says the reading stopped at `maxFiles`; entities of unread files are never reported as unconnected.
 */
export async function analyzeUnlinked(source: UnlinkedSource, options: AnalyzeOptions = {}): Promise<UnlinkedReport> {
  const maxFiles = options.maxFiles ?? 600;
  const drives = await source.drives();
  const all: EngineFileSummary[] = [];
  let complete = drives.ok;
  let limited = false;
  if (drives.ok) {
    for (const drive of drives.value) {
      if (drive.status === "deleted" || options.signal?.aborted) continue;
      let cursor: string | undefined;
      for (;;) {
        const page = await source.files({ driveId: drive.id, ...(cursor ? { cursor } : {}) });
        if (!page.ok) { complete = false; break; }
        all.push(...page.value.items.filter((file) => !IGNORED.has(file.state)));
        if (!page.value.next_cursor) break;
        cursor = page.value.next_cursor;
      }
    }
  }
  const linkedPaths = await source.linkedFilePaths().catch(() => new Set<string>());
  const ready = all.filter((file) => READY.has(file.state));
  const toRead = ready.slice(0, maxFiles);
  limited = ready.length > toRead.length;
  const entitiesOf = new Map<string, EntityRefLike[]>();
  const edges: [string, string][] = [];
  /** Per file: the things it holds, other than its own Document entity (the engine names one after every file). */
  const fileEntities = new Map<string, Set<string>>();
  await pool(toRead, options.concurrency ?? 6, async (file) => {
    const detail = await source.detail(file.id);
    if (!detail.ok) { complete = false; return; }
    const parts = detailParts(detail.value);
    const keys = new Set<string>();
    const note = (entity: EntityRefLike | null | undefined) => {
      if (entity?.key && entity.type !== "Document") { keys.add(entity.key); if (!entitiesOf.has(entity.key)) entitiesOf.set(entity.key, [entity]); }
    };
    for (const entity of parts.entities ?? []) note(entity);
    for (const fact of parts.facts ?? []) {
      const live = fact.status !== "retracted" && fact.status !== "rejected";
      note(fact.subject);
      if (live) note(fact.object);
      if (live && fact.subject?.key && fact.object?.key && fact.subject.type !== "Document" && fact.object.type !== "Document" && fact.subject.key !== fact.object.key) edges.push([fact.subject.key, fact.object.key]);
    }
    fileEntities.set(file.id, keys);
  }, options.signal);

  // Which files hold each thing. A file is connected when one of its things is held by another file (or the file links to a
  // table row); an entity is connected when the island of related entities it sits on touches two files or a table row.
  const byId = new Map(all.map((file) => [file.id, file]));
  const linkedFile = (id: string) => { const file = byId.get(id); return !!file && linkedPaths.has(`${file.drive_id}|${file.path}`); };
  const holders = new Map<string, Set<string>>();
  for (const [fileId, keys] of fileEntities) for (const key of keys) holders.set(key, (holders.get(key) ?? new Set()).add(fileId));
  const parent = new Map<string, string>();
  const find = (key: string): string => { let root = key; while ((parent.get(root) ?? root) !== root) root = parent.get(root)!; parent.set(key, root); return root; };
  for (const key of holders.keys()) parent.set(key, key);
  for (const [a, b] of edges) if (holders.has(a) && holders.has(b)) parent.set(find(a), find(b));
  const islandFiles = new Map<string, Set<string>>();
  for (const [key, ids] of holders) { const root = find(key); const set = islandFiles.get(root) ?? new Set<string>(); for (const id of ids) set.add(id); islandFiles.set(root, set); }
  const islandReaches = (root: string) => { const ids = islandFiles.get(root)!; return ids.size > 1 || [...ids].some(linkedFile); };

  const files: UnlinkedFile[] = [];
  for (const file of all) {
    const keys = fileEntities.get(file.id);
    // Cut short: what other files hold is not known, so a ready file is never called unconnected on that evidence.
    const shared = !keys || limited || fileEntities.size < 2 || linkedFile(file.id) || [...keys].some((key) => (holders.get(key)?.size ?? 0) > 1 || islandReaches(find(key)));
    const reason = fileReason(file, keys ? { entities: keys.size, connected: shared } : null);
    if (reason) files.push({ fileId: file.id, driveId: file.drive_id, path: file.path, name: file.path.slice(file.path.lastIndexOf("/") + 1), reason });
  }
  const entities: UnlinkedEntity[] = [];
  if (!limited && fileEntities.size > 1) for (const [key, refs] of entitiesOf) {
    if (!holders.has(key) || islandReaches(find(key))) continue;
    const ref = refs[0]!;
    const fileIds = [...holders.get(key)!];
    entities.push({ key, name: ref.name ?? key, type: ref.type ?? "Thing", fileIds, fileNames: fileIds.map((id) => byId.get(id)?.path.slice(byId.get(id)!.path.lastIndexOf("/") + 1) ?? id) });
  }
  files.sort((a, b) => a.path.localeCompare(b.path));
  entities.sort((a, b) => a.name.localeCompare(b.name));
  return { files, entities, totalFiles: all.length, checkedFiles: toRead.length, read: [...fileEntities.keys()], limited, complete };
}

/** The reading of the real bridge. */
export const bridgeUnlinkedSource: UnlinkedSource = {
  drives: async () => {
    const result = await invoke("drives-list", {});
    return result.ok ? { ok: true, value: result.value.map((d) => ({ id: d.id, name: d.name, status: d.status })) } : result;
  },
  files: async ({ driveId, cursor }) => {
    const result = await invoke("files-list", { driveId, recursive: true, limit: 500, ...(cursor ? { cursor } : {}) });
    return result.ok ? { ok: true, value: { items: result.value.items as EngineFileSummary[], next_cursor: result.value.next_cursor ?? null } } : result;
  },
  detail: (fileId) => invoke("file-detail", { fileId, include: ["entities", "facts"] }),
  linkedFilePaths: async () => {
    const result = await invoke("atlas-links", {});
    const paths = new Set<string>();
    if (result.ok) for (const item of (result.value as { items?: { drive_id?: string; status?: string; provenance?: { file?: { path?: string }[] } }[] }).items ?? []) {
      if (item.status === "rejected") continue;
      for (const file of item.provenance?.file ?? []) if (file.path) paths.add(`${item.drive_id}|${file.path}`);
    }
    return paths;
  },
};

// ---- a shared, cached reading so Home, Sources and the atlas do not each read every file ----------------------------------------

type Cached = { key: string; at: number; report: UnlinkedReport };
let cached: Cached | null = null;
let inflight: { key: string; promise: Promise<UnlinkedReport> } | null = null;
const MAX_AGE_MS = 60_000;

/** One reading per `key` (the ontology version and the file count are a good key); concurrent callers share it. */
export function readUnlinked(key: string, source: UnlinkedSource = bridgeUnlinkedSource, { force = false }: { force?: boolean } = {}): Promise<UnlinkedReport> {
  if (!force && cached && cached.key === key && Date.now() - cached.at < MAX_AGE_MS) return Promise.resolve(cached.report);
  if (inflight && inflight.key === key) return inflight.promise;
  const promise = analyzeUnlinked(source).then((report) => { cached = { key, at: Date.now(), report }; return report; }).finally(() => { if (inflight?.promise === promise) inflight = null; });
  inflight = { key, promise };
  return promise;
}
export const resetUnlinked = () => { cached = null; inflight = null; };

export type UnlinkedState = { status: "loading" | "ready" | "error"; report: UnlinkedReport | null };

/** The report for the current data, `loading` the first time. A file or version change (`key`) reads again. */
export function useUnlinked(key: string, { enabled = true, source = bridgeUnlinkedSource }: { enabled?: boolean; source?: UnlinkedSource } = {}): UnlinkedState {
  const [state, setState] = useState<UnlinkedState>({ status: "loading", report: cached?.report ?? null });
  useEffect(() => {
    if (!enabled) return undefined;
    let live = true;
    void readUnlinked(key, source).then((report) => { if (live) setState({ status: "ready", report }); }).catch(() => { if (live) setState((s) => ({ status: "error", report: s.report })); });
    return () => { live = false; };
  }, [key, enabled, source]);
  return state;
}

/** The sentence of a reason, in the language of the app. */
export function useUnlinkedText(): (reason: UnlinkedReason) => string {
  const { t } = useLocalization();
  return (reason) => {
    switch (reason.kind) {
      case "unsupported": return t("app.unlinked.reason.unsupported", { format: reason.format });
      case "parse_failed": return t("app.unlinked.reason.parse_failed", { code: reason.code });
      case "no_relations": return t("app.unlinked.reason.no_relations", { count: reason.count });
      case "entity": return reason.file ? t("app.unlinked.reason.entity_in", { file: reason.file }) : t("app.unlinked.reason.entity");
      default: return t(`app.unlinked.reason.${reason.kind}`);
    }
  };
}

/** The reason a file row shows: the report's when the file was read, else what its own state says (unsupported, still processing, ...). */
export function reasonForRow(file: FileLike & { id: string }, report: UnlinkedReport | null): UnlinkedReason | null {
  if (report) {
    const found = report.files.find((entry) => entry.fileId === file.id);
    if (found) return found.reason;
    if (report.read.includes(file.id)) return null;
  }
  return fileReason(file, null);
}
