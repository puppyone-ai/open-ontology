import type { ReviewBySource, ReviewDecisionItem } from "../../shared/super-ontology/bridge";
import type { ContextPack, Fact, ModelConfig, ReviewGroup, ReviewItem, RevokeResult, Share, ShareUpdate, Usage } from "./contract/docsApi.generated";
import type { ApiResult } from "./bridgeInvoke";
import { toSuperDriveError, type SuperDriveError } from "./superDriveErrors";
import type { OntologyApi } from "./superDriveOntologyApi";
import { createEmptyReviewList, reviewKey, type Notice, type ReviewList, type ShareOperation, type SuperDriveState } from "./superDriveState";
import type { EntityDetail } from "./superDriveOntologyTypes";

export type OntologyActionResult<T = void> = { ok: true; value: T } | { ok: false; error: SuperDriveError; operationId?: string };

const REVIEW_PAGE_LIMIT = 200;
const SHARE_PAGE_LIMIT = 200;
const SHARE_PAGE_CAP = 20;

export type ReviewOutcome = {
  accepted: number;
  rejected: number;
  /** Items the engine had already decided (someone else, or an earlier request) - removed without a fuss. */
  alreadyDecided: number;
  /** Items that changed since they were loaded: shown again after a reload. */
  conflicts: number;
  notFound: number;
  invalid: number;
  publishPending: boolean;
};

type Context = {
  api: OntologyApi;
  getState: () => SuperDriveState;
  setState: (update: (current: SuperDriveState) => SuperDriveState) => void;
  createId: (prefix?: string) => string;
  addNotice: (notice: Omit<Notice, "id" | "at">, ttlMs?: number) => void;
  /** Where the wait for a publish ends when the engine publishes nothing (a decision that needed no new version). */
  scheduler?: { setTimeout: (callback: () => void, ms: number) => unknown };
};

/** The engine publishes within 2 s of a decision (10 section 8.3); a page still saying "publishing" after this reads the version again. */
const PUBLISH_WAIT_MS = 6_000;

const fail = <T,>(result: Extract<ApiResult<unknown>, { ok: false }>, operationId?: string): OntologyActionResult<T> => ({ ok: false, error: toSuperDriveError(result), ...(operationId ? { operationId } : {}) });
const ok = <T,>(value: T): OntologyActionResult<T> => ({ ok: true, value });

/** Chains of refreshed Shares: root (oldest) id -> ids oldest first, from `refreshed_from` links. */
export function buildShareChains(shares: readonly Share[]): Record<string, string[]> {
  const byId = new Map(shares.map((share) => [share.id, share]));
  const rootOf = (share: Share): string => {
    let current = share;
    const seen = new Set<string>([current.id]);
    for (;;) {
      const parent = current.refreshed_from ? byId.get(current.refreshed_from) : undefined;
      if (!parent || seen.has(parent.id)) return current.refreshed_from && !parent ? current.refreshed_from : current.id;
      seen.add(parent.id);
      current = parent;
    }
  };
  const chains: Record<string, string[]> = {};
  const ordered = [...shares].sort((left, right) => (left.created_at < right.created_at ? -1 : left.created_at > right.created_at ? 1 : 0));
  for (const share of ordered) {
    if (!share.refreshed_from) continue;
    const root = rootOf(share);
    (chains[root] ??= root !== share.id && byId.has(root) ? [root] : []).push(share.id);
  }
  return chains;
}

/** Newest first, as the engine lists them; keeps arrival order for equal times. */
const byCreatedDesc = (left: Share, right: Share) => (left.created_at < right.created_at ? 1 : left.created_at > right.created_at ? -1 : 0);

/**
 * Actions of the L4 and L6 loops (reviews, entities, shares, usage, model). They share the state and the notice
 * mechanism of `createSuperDriveStore` through `Context`; nothing here reaches the bridge except through `OntologyApi`.
 */
export function createOntologyActions({ api, getState, setState, createId, addNotice, scheduler = { setTimeout: (callback, ms) => setTimeout(callback, ms) } }: Context) {
  const patchReview = (update: (review: SuperDriveState["review"]) => SuperDriveState["review"]) => setState((current) => ({ ...current, review: update(current.review) }));
  const patchShares = (update: (shares: SuperDriveState["shares"]) => SuperDriveState["shares"]) => setState((current) => ({ ...current, shares: update(current.shares) }));
  /** A decision may publish a version; when the version event does not come (nothing to publish) the flag is cleared after the wait. */
  function expectPublish(key: string) {
    patchReview((review) => ({ ...review, publishing: { ...review.publishing, [key]: true } }));
    scheduler.setTimeout(() => {
      void loadOntologyVersion(null).then(() => patchReview((review) => ({ ...review, publishing: { ...review.publishing, [key]: false, "*": false } })));
    }, PUBLISH_WAIT_MS);
  }
  const requestCounter = new Map<string, number>();
  const nextRequest = (key: string) => { const next = (requestCounter.get(key) ?? 0) + 1; requestCounter.set(key, next); return next; };
  const isLatest = (key: string, token: number) => requestCounter.get(key) === token;

  // ---- review queue -------------------------------------------------------------------

  function putGroups(driveId: string, update: (list: ReviewList<ReviewGroup>) => ReviewList<ReviewGroup>) {
    patchReview((review) => ({ ...review, groups: { ...review.groups, [driveId]: update(review.groups[driveId] ?? createEmptyReviewList()) } }));
  }
  function putList(key: string, update: (list: ReviewList<ReviewItem>) => ReviewList<ReviewItem>) {
    patchReview((review) => ({ ...review, lists: { ...review.lists, [key]: update(review.lists[key] ?? createEmptyReviewList()) } }));
  }

  /** Pending items grouped by source file (the default queue view); a stale listing is read again. */
  async function loadReviewGroups(driveId: string, { force = false }: { force?: boolean } = {}): Promise<void> {
    const existing = getState().review.groups[driveId];
    if (existing?.loading) return;
    if (existing?.loaded && !existing.stale && !force) return;
    const token = nextRequest(`groups:${driveId}`);
    putGroups(driveId, (list) => ({ ...list, loading: true, error: null }));
    const result = await api.reviewGroups({ driveId, limit: REVIEW_PAGE_LIMIT });
    if (!isLatest(`groups:${driveId}`, token)) return;
    if (!result.ok) { putGroups(driveId, (list) => ({ ...list, loading: false, error: toSuperDriveError(result) })); return; }
    putGroups(driveId, () => ({ items: result.value.items, nextCursor: result.value.next_cursor, asOfSeq: result.value.as_of_seq, loaded: true, loading: false, stale: false, error: null }));
  }

  async function loadMoreReviewGroups(driveId: string): Promise<void> {
    const list = getState().review.groups[driveId];
    if (!list || list.loading || list.nextCursor === null) return;
    putGroups(driveId, (current) => ({ ...current, loading: true }));
    const result = await api.reviewGroups({ driveId, limit: REVIEW_PAGE_LIMIT, cursor: list.nextCursor });
    if (!result.ok) { putGroups(driveId, (current) => ({ ...current, loading: false, error: toSuperDriveError(result) })); return; }
    putGroups(driveId, (current) => ({ ...current, items: [...current.items, ...result.value.items], nextCursor: result.value.next_cursor, loading: false }));
  }

  /** Pending items of one source file, or of one kind ("by kind" view, when `source` is null). */
  async function loadReviewItems(input: { driveId: string; source?: string | null; kind?: string | null; force?: boolean }): Promise<void> {
    const key = reviewKey(input.driveId, input.source ?? null, input.kind ?? null);
    const existing = getState().review.lists[key];
    if (existing?.loading) return;
    if (existing?.loaded && !existing.stale && !input.force) return;
    const token = nextRequest(`items:${key}`);
    putList(key, (list) => ({ ...list, loading: true, error: null }));
    const result = await api.reviewItems({ driveId: input.driveId, source: input.source ?? undefined, kind: input.kind ?? undefined, status: "pending", limit: REVIEW_PAGE_LIMIT });
    if (!isLatest(`items:${key}`, token)) return;
    if (!result.ok) { putList(key, (list) => ({ ...list, loading: false, error: toSuperDriveError(result) })); return; }
    putList(key, () => ({ items: result.value.items, nextCursor: result.value.next_cursor, asOfSeq: result.value.as_of_seq, loaded: true, loading: false, stale: false, error: null }));
  }

  async function loadMoreReviewItems(input: { driveId: string; source?: string | null; kind?: string | null }): Promise<void> {
    const key = reviewKey(input.driveId, input.source ?? null, input.kind ?? null);
    const list = getState().review.lists[key];
    if (!list || list.loading || list.nextCursor === null) return;
    putList(key, (current) => ({ ...current, loading: true }));
    const result = await api.reviewItems({ driveId: input.driveId, source: input.source ?? undefined, kind: input.kind ?? undefined, status: "pending", limit: REVIEW_PAGE_LIMIT, cursor: list.nextCursor });
    if (!result.ok) { putList(key, (current) => ({ ...current, loading: false, error: toSuperDriveError(result) })); return; }
    putList(key, (current) => {
      const known = new Set(current.items.map((item) => item.id));
      return { ...current, items: [...current.items, ...result.value.items.filter((item) => !known.has(item.id))], nextCursor: result.value.next_cursor, loading: false };
    });
  }

  function removeReviewItems(ids: ReadonlySet<string>) {
    if (ids.size === 0) return;
    patchReview((review) => {
      const lists: typeof review.lists = {};
      for (const [key, list] of Object.entries(review.lists)) lists[key] = list.items.some((item) => ids.has(item.id)) ? { ...list, items: list.items.filter((item) => !ids.has(item.id)) } : list;
      return { ...review, lists };
    });
  }

  function summarise(results: readonly { id?: string; status?: string }[], accepted: number, rejected: number, publishPending: boolean): { outcome: ReviewOutcome; removed: Set<string>; refetch: Set<string> } {
    const outcome: ReviewOutcome = { accepted, rejected, alreadyDecided: 0, conflicts: 0, notFound: 0, invalid: 0, publishPending };
    const removed = new Set<string>();
    const refetch = new Set<string>();
    for (const entry of results) {
      const id = entry.id;
      switch (entry.status) {
        case "ok": if (id) removed.add(id); break;
        case "already_decided": outcome.alreadyDecided += 1; if (id) removed.add(id); break;
        case "not_found": outcome.notFound += 1; if (id) removed.add(id); break;
        case "conflict": outcome.conflicts += 1; if (id) refetch.add(id); break;
        case "invalid": outcome.invalid += 1; break;
        default: break;
      }
    }
    return { outcome, removed, refetch };
  }

  function afterDecision(driveId: string, outcome: ReviewOutcome, removed: Set<string>, refetch: Set<string>) {
    removeReviewItems(removed);
    patchReview((review) => {
      const groups = review.groups[driveId];
      const lists: typeof review.lists = { ...review.lists };
      // Anything that conflicted is read again; the group counts always are (they were cut by what was removed).
      if (refetch.size > 0) for (const [key, list] of Object.entries(lists)) if (list.items.some((item) => refetch.has(item.id))) lists[key] = { ...list, stale: true };
      return {
        ...review, lists,
        groups: groups ? { ...review.groups, [driveId]: { ...groups, stale: true } } : review.groups,
        publishing: review.publishing,
      };
    });
    if (outcome.publishPending) expectPublish(driveId);
    addNotice({ kind: "review_result", values: { ...outcome } }, 6_000);
  }

  /** Decides items one by one (accept, reject, edit); the main process cuts batches of 500. */
  async function decideReviewItems(input: { driveId: string; items: ReviewDecisionItem[]; operationId?: string }): Promise<OntologyActionResult<ReviewOutcome>> {
    if (input.items.length === 0) return ok({ accepted: 0, rejected: 0, alreadyDecided: 0, conflicts: 0, notFound: 0, invalid: 0, publishPending: false });
    const operationId = input.operationId ?? createId("rv");
    const result = await api.decideReview({ operationId, items: input.items });
    if (!result.ok) return fail(result, operationId);
    const { outcome, removed, refetch } = summarise(result.value.results ?? [], result.value.accepted ?? 0, result.value.rejected ?? 0, result.value.publish_pending === true);
    afterDecision(input.driveId, outcome, removed, refetch);
    return ok(outcome);
  }

  /** Accepts or rejects everything queued for one source before `asOfSeq`: one operation, one IPC call. */
  async function decideReviewBySource(input: { driveId: string; fileId: string; decision: "accept" | "reject"; kinds?: string[]; asOfSeq?: number; operationId?: string }): Promise<OntologyActionResult<ReviewOutcome>> {
    const operationId = input.operationId ?? createId("rv");
    const bySource: ReviewBySource = { fileId: input.fileId, decision: input.decision, ...(input.kinds ? { kinds: input.kinds } : {}), ...(input.asOfSeq === undefined ? {} : { asOfSeq: input.asOfSeq }) };
    const result = await api.decideReview({ operationId, bySource });
    if (!result.ok) return fail(result, operationId);
    const { outcome, removed, refetch } = summarise(result.value.results ?? [], result.value.accepted ?? 0, result.value.rejected ?? 0, result.value.publish_pending === true);
    // The group is gone whatever the per-item results say: it is read again and shows what is left.
    patchReview((review) => {
      const groups = review.groups[input.driveId];
      return groups ? { ...review, groups: { ...review.groups, [input.driveId]: { ...groups, items: groups.items.filter((group) => (group.source as { file_id?: string } | undefined)?.file_id !== input.fileId || (input.kinds !== undefined && group.kind !== undefined && !input.kinds.includes(group.kind))) } } } : review;
    });
    afterDecision(input.driveId, outcome, removed, refetch);
    return ok(outcome);
  }

  // ---- entities -----------------------------------------------------------------------

  async function loadEntity(entityIdOrKey: string, { includeHistory = true, force = false }: { includeHistory?: boolean; force?: boolean } = {}): Promise<OntologyActionResult<EntityDetail>> {
    const cacheKey = `${entityIdOrKey}|${includeHistory ? "h" : ""}`;
    const cached = getState().entities.get(cacheKey) as EntityDetail | undefined;
    if (cached && !force) return ok(cached);
    const result = await api.entityDetail({ entityIdOrKey, includeHistory });
    if (!result.ok) return fail(result);
    getState().entities.set(cacheKey, result.value);
    setState((current) => ({ ...current }));
    return ok(result.value);
  }

  function forgetEntities() {
    getState().entities.clear();
    setState((current) => ({ ...current }));
  }

  /** A correction is a write with If-Match: the next version publishes it (D59). */
  async function correctFact(input: { driveId: string | null; factId: string; revision: number; action: "mark_wrong" | "set_value"; value?: Record<string, unknown>; reason?: string; operationId?: string }): Promise<OntologyActionResult<Fact>> {
    const operationId = input.operationId ?? createId("fx");
    const result = await api.correctFact({ operationId, factId: input.factId, revision: input.revision, action: input.action, value: input.value, reason: input.reason });
    forgetEntities();
    if (!result.ok) return fail(result, operationId);
    expectPublish(input.driveId ?? "*");
    return ok(result.value);
  }

  async function mergeEntities(input: { driveId: string | null; into: string; from: string[]; revision: number; operationId?: string }): Promise<OntologyActionResult<EntityDetail>> {
    const operationId = input.operationId ?? createId("mg");
    const result = await api.mergeEntities({ operationId, into: input.into, from: input.from, revision: input.revision });
    forgetEntities();
    if (!result.ok) return fail(result, operationId);
    expectPublish(input.driveId ?? "*");
    return ok(result.value);
  }

  async function splitEntity(input: { driveId: string | null; entityId: string; revision: number; groups: { aliases?: string[]; facts?: string[] }[]; operationId?: string }) {
    const operationId = input.operationId ?? createId("sp");
    const result = await api.splitEntity({ operationId, entityId: input.entityId, revision: input.revision, groups: input.groups });
    forgetEntities();
    if (!result.ok) return fail(result, operationId);
    expectPublish(input.driveId ?? "*");
    return ok(result.value);
  }

  // ---- shares -------------------------------------------------------------------------

  function setPending(shareId: string, operation: ShareOperation | null) {
    patchShares((shares) => {
      const pending = { ...shares.pending };
      if (operation) pending[shareId] = operation; else delete pending[shareId];
      return { ...shares, pending };
    });
  }

  function upsertShare(share: Share) {
    patchShares((shares) => {
      const known = shares.byId[share.id] !== undefined;
      const byId = { ...shares.byId, [share.id]: share };
      const order = known ? shares.order : [share.id, ...shares.order];
      return { ...shares, byId, order, chains: buildShareChains(Object.values(byId)) };
    });
  }

  /** Every Share of the Drive (the main process pages; at most 20 pages of 200), newest first, with the refresh chains rebuilt. */
  async function loadShares({ driveId, force = false }: { driveId?: string; force?: boolean } = {}): Promise<OntologyActionResult<Share[]>> {
    const current = getState().shares;
    if (current.loading) return ok(current.order.map((id) => current.byId[id]));
    if (current.loaded && !force) return ok(current.order.map((id) => current.byId[id]));
    patchShares((shares) => ({ ...shares, loading: true, error: null }));
    const collected: Share[] = [];
    let cursor: string | undefined;
    for (let page = 0; page < SHARE_PAGE_CAP; page += 1) {
      const result = await api.listShares({ driveId, limit: SHARE_PAGE_LIMIT, cursor });
      if (!result.ok) {
        const error = toSuperDriveError(result);
        patchShares((shares) => ({ ...shares, loading: false, error }));
        return { ok: false, error };
      }
      collected.push(...result.value.items);
      if (!result.value.next_cursor) break;
      cursor = result.value.next_cursor;
    }
    const sorted = collected.sort(byCreatedDesc);
    patchShares((shares) => {
      const byId: Record<string, Share> = {};
      for (const share of sorted) byId[share.id] = share;
      return { ...shares, byId, order: sorted.map((share) => share.id), chains: buildShareChains(sorted), loaded: true, loading: false, stale: false, error: null };
    });
    return ok(sorted);
  }

  async function loadShare(shareId: string): Promise<OntologyActionResult<Share>> {
    const result = await api.getShare({ shareId });
    if (!result.ok) return fail(result);
    upsertShare(result.value);
    return ok(result.value);
  }

  /** The pack the engine would build for these settings: a read-only preview (nothing is mounted). */
  async function previewPack(compile: Parameters<OntologyApi["compilePack"]>[0]["compile"]): Promise<OntologyActionResult<ContextPack>> {
    const result = await api.compilePack({ operationId: createId("pk"), compile });
    return result.ok ? ok(result.value) : fail(result);
  }

  async function readPack(packId: string): Promise<OntologyActionResult<ContextPack>> {
    const result = await api.readPack({ packId });
    return result.ok ? ok(result.value) : fail(result);
  }

  /** One-click mount: the engine writes the folder and answers with the Share (a timed-out call is repeated with the same operation id). */
  async function mountShare(input: { compile: Parameters<OntologyApi["compilePack"]>[0]["compile"]; targetGrantId: string; expiresInSeconds: number; group?: string; refreshPolicy?: "manual" | "at-checkpoint" | "pinned"; operationId?: string }): Promise<OntologyActionResult<Share>> {
    const operationId = input.operationId ?? createId("sh");
    const result = await api.mountShare({ operationId, compile: input.compile, targetGrantId: input.targetGrantId, expiresInSeconds: input.expiresInSeconds, group: input.group, refreshPolicy: input.refreshPolicy });
    if (!result.ok) return fail(result, operationId);
    upsertShare(result.value);
    return ok(result.value);
  }

  async function refreshShare(shareId: string, operationId?: string): Promise<OntologyActionResult<Share>> {
    const op = operationId ?? createId("rf");
    setPending(shareId, "refreshing");
    const result = await api.refreshShare({ operationId: op, shareId });
    setPending(shareId, null);
    if (!result.ok) return fail(result, op);
    const fresh = result.value;
    // The old Share ends as `refreshed` and links to the new one; the event says the same a moment later.
    patchShares((shares) => {
      const old = shares.byId[shareId];
      const byId = { ...shares.byId, [fresh.id]: fresh, ...(old && old.state !== "refreshed" ? { [shareId]: { ...old, state: "refreshed" as const, drift: null } } : {}) };
      return { ...shares, byId, order: shares.order.includes(fresh.id) ? shares.order : [fresh.id, ...shares.order], chains: buildShareChains(Object.values(byId)) };
    });
    return ok(fresh);
  }

  async function revokeShare(shareId: string, { reason, operationId }: { reason?: string; operationId?: string } = {}): Promise<OntologyActionResult<RevokeResult>> {
    const op = operationId ?? createId("rk");
    setPending(shareId, "revoking");
    const result = await api.revokeShare({ operationId: op, shareId, reason });
    setPending(shareId, null);
    if (!result.ok) return fail(result, op);
    const revoked = result.value.share;
    if (revoked) upsertShare(revoked);
    else patchShares((shares) => (shares.byId[shareId] ? { ...shares, byId: { ...shares.byId, [shareId]: { ...shares.byId[shareId], state: "revoked" } } } : shares));
    return ok(result.value);
  }

  async function revealShare(shareId: string) {
    const result = await api.revealShare({ shareId });
    return result.ok ? ok(result.value.revealed) : fail<boolean>(result);
  }

  async function loadShareUpdates(shareId: string, { force = false }: { force?: boolean } = {}): Promise<OntologyActionResult<ShareUpdate[]>> {
    const current = getState().shares.updates[shareId];
    if (current?.loaded && !current.stale && !force) return ok(current.items);
    const result = await api.shareUpdates({ shareId, limit: 50 });
    if (!result.ok) return fail(result);
    const items = [...result.value.items].sort((left, right) => right.seq - left.seq);
    patchShares((shares) => ({ ...shares, updates: { ...shares.updates, [shareId]: { items, loaded: true, stale: false, latestLevel: items[0]?.level ?? null } } }));
    return ok(items);
  }

  /** The whole task group at once; a member that fails is named in the answer (D55). */
  async function refreshShareGroup(groupId: string, operationId?: string) {
    const op = operationId ?? createId("gr");
    const result = await api.refreshShareGroup({ operationId: op, groupId });
    if (!result.ok) return fail<{ failed: string[] }>(result, op);
    void loadShares({ force: true });
    const failed = result.value.results.filter((entry) => entry.status !== "ok").map((entry) => entry.id);
    return ok({ failed });
  }

  // ---- model, usage, versions ----------------------------------------------------------

  async function loadModelConfig({ force = false }: { force?: boolean } = {}): Promise<OntologyActionResult<ModelConfig>> {
    const current = getState().modelConfig;
    if (current.config && current.loaded && !force) return ok(current.config);
    setState((state) => ({ ...state, modelConfig: { ...state.modelConfig, loading: true, error: null } }));
    const result = await api.readModelConfig();
    if (!result.ok) {
      const error = toSuperDriveError(result);
      setState((state) => ({ ...state, modelConfig: { ...state.modelConfig, loading: false, error } }));
      return { ok: false, error };
    }
    setState((state) => ({ ...state, modelConfig: { config: result.value, loaded: true, loading: false, error: null } }));
    return ok(result.value);
  }

  async function saveModelConfig(input: { provider: "openai-compatible" | "anthropic"; baseUrl: string; model: string; apiKey?: string }): Promise<OntologyActionResult<ModelConfig>> {
    const revision = getState().modelConfig.config?.revision ?? 0;
    const result = await api.saveModelConfig({ ...input, revision });
    if (!result.ok) {
      if (result.error.code === "DOCS_ENGINE_PRECONDITION_FAILED") void loadModelConfig({ force: true });
      return fail(result);
    }
    setState((state) => ({ ...state, modelConfig: { config: result.value, loaded: true, loading: false, error: null } }));
    return ok(result.value);
  }

  async function loadUsage(driveId: string): Promise<OntologyActionResult<Usage>> {
    const result = await api.readUsage({ driveId, days: 7 });
    if (!result.ok) return fail(result);
    const today = (result.value.drives ?? []).filter((row) => row.drive_id === driveId).sort((left, right) => ((left.day ?? "") < (right.day ?? "") ? 1 : -1))[0];
    if (today) {
      const limit = typeof today.limit === "number" ? today.limit : null;
      const calls = typeof today.calls === "number" ? today.calls : 0;
      setState((state) => ({
        ...state,
        usage: { ...state.usage, [driveId]: { day: today.day ?? "", calls, limit, limited: limit !== null && limit > 0 && calls >= limit, cacheHits: today.cache_hits, filesLimited: today.files_limited } },
      }));
    }
    return ok(result.value);
  }

  async function loadOntologyVersion(driveId: string | null): Promise<void> {
    const result = await api.ontologyVersions({ limit: 1 });
    if (!result.ok || result.value.items.length === 0) return;
    const [newest] = result.value.items;
    setState((state) => ({ ...state, ontology: { ...state.ontology, [driveId ?? "*"]: { n: newest.n, asOf: newest.as_of } } }));
  }

  async function loadConstraints({ force = false }: { force?: boolean } = {}) {
    const current = getState().constraints;
    if (current.loaded && !current.stale && !force) return;
    const result = await api.listConstraints({ limit: 100 });
    if (!result.ok) return;
    setState((state) => ({ ...state, constraints: { items: result.value.items, loaded: true, stale: false } }));
  }

  /** Decodes the recording with the macOS helper, then asks the engine to parse it again (the engine reads the output from its exchange directory). */
  async function decodeMediaOnHost(fileId: string): Promise<OntologyActionResult<{ durationMs: number; frames: number }>> {
    const decoded = await api.decodeMedia({ fileId });
    if (!decoded.ok) return fail(decoded);
    return ok({ durationMs: decoded.value.durationMs, frames: decoded.value.frames });
  }

  return {
    decodeMediaOnHost, cancelMediaDecode: (fileId: string) => api.cancelMediaDecode({ fileId }),
    loadReviewGroups, loadMoreReviewGroups, loadReviewItems, loadMoreReviewItems, decideReviewItems, decideReviewBySource,
    loadEntity, searchEntities: api.searchEntities, resolveText: api.resolveText, correctFact, mergeEntities, splitEntity,
    loadShares, loadShare, previewPack, readPack, mountShare, refreshShare, revokeShare, revealShare, loadShareUpdates, refreshShareGroup,
    loadModelConfig, saveModelConfig, loadUsage, loadOntologyVersion, loadConstraints,
  };
}

export type OntologyActions = ReturnType<typeof createOntologyActions>;
