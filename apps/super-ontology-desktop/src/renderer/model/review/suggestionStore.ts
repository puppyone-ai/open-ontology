import type { Suggestion, SuggestionDigest, SuggestionDiff } from "../contract/docsApi.generated";
import type { SuperDriveApi } from "../superDriveApi";
import { toSuperDriveError, type SuperDriveError } from "../superDriveErrors";
import { digestView, type DigestView } from "./suggestionModel";

export type SuggestionState = {
  day: string | null;
  digest: SuggestionDigest | null;
  view: DigestView | null;
  loading: boolean;
  error: SuperDriveError | null;
  /** Suggestions with a decision in flight (their buttons are off). */
  busy: ReadonlySet<string>;
  /** What the last decision did to files, for the confirmation line (`written` counts files Desktop wrote). */
  lastOutcome: { kind: "decided" | "source"; decided: number; failed: number; written: number; code?: string } | null;
};

export type SuggestionActionResult<T = void> = { ok: true; value: T } | { ok: false; error: SuperDriveError };

/**
 * State and actions of the suggestion views (F-S7-01, F-S8-01): the day's digest, one decision, one decision for a whole source,
 * and the diff of one suggestion. Decisions show their result at once (the suggestion leaves the pending list or the in-effect list)
 * and are put back when the engine refuses. Kept apart from the main store: it only needs the API.
 */
export function createSuggestionStore({ api }: { api: Pick<SuperDriveApi, "suggestionDigest" | "suggestionDiff" | "decideSuggestion" | "decideSuggestionsBySource"> }) {
  let state: SuggestionState = { day: null, digest: null, view: null, loading: false, error: null, busy: new Set(), lastOutcome: null };
  const listeners = new Set<() => void>();
  let requestCounter = 0;
  const set = (patch: Partial<SuggestionState>) => {
    state = { ...state, ...patch };
    for (const listener of [...listeners]) listener();
  };
  const withDigest = (digest: SuggestionDigest, extra: Partial<SuggestionState> = {}) => set({ digest, view: digestView(digest), day: digest.day, ...extra });

  async function load(day?: string): Promise<SuggestionActionResult<SuggestionDigest>> {
    const token = ++requestCounter;
    set({ loading: true, error: null });
    const result = await api.suggestionDigest(day);
    if (token !== requestCounter) return result.ok ? { ok: true, value: result.value } : { ok: false, error: toSuperDriveError(result) };
    if (!result.ok) {
      const error = toSuperDriveError(result);
      set({ loading: false, error });
      return { ok: false, error };
    }
    withDigest(result.value, { loading: false });
    return { ok: true, value: result.value };
  }

  function mark(ids: ReadonlySet<string>, patch: (suggestion: Suggestion) => Suggestion | null): SuggestionDigest | null {
    const digest = state.digest;
    if (!digest) return null;
    const apply = (items: Suggestion[]) => items.flatMap((item) => (ids.has(item.id) ? (patch(item) ? [patch(item) as Suggestion] : []) : [item]));
    return { ...digest, in_effect: apply(digest.in_effect), pending: apply(digest.pending) };
  }

  async function decide(suggestion: Suggestion, decision: "accept" | "reject" | "undo"): Promise<SuggestionActionResult<Suggestion>> {
    const before = state.digest;
    set({ busy: new Set([...state.busy, suggestion.id]), error: null });
    // Show the result before the engine answers: accepted and rejected leave the pending list, an undone one leaves the in-effect list.
    const optimistic = mark(new Set([suggestion.id]), () => null);
    if (optimistic) withDigest(optimistic);
    const result = await api.decideSuggestion({ id: suggestion.id, decision, revision: suggestion.revision });
    const busy = new Set(state.busy);
    busy.delete(suggestion.id);
    if (!result.ok) {
      const error = toSuperDriveError(result);
      if (before) withDigest(before, { busy, error });
      else set({ busy, error });
      return { ok: false, error };
    }
    set({ busy, lastOutcome: { kind: "decided", decided: 1, failed: 0, written: result.value.writeback?.written.length ?? 0 } });
    await load(state.day ?? undefined);
    return { ok: true, value: result.value.suggestion };
  }

  /** "Accept (or reject) everything from this source": one operation for the person, one engine decision per suggestion. */
  async function decideSource(shareId: string, decision: "accept" | "reject"): Promise<SuggestionActionResult<{ decided: number; failed: number }>> {
    const before = state.digest;
    const ids = new Set((state.digest?.pending ?? []).filter((item) => item.share_id === shareId).map((item) => item.id));
    set({ busy: new Set([...state.busy, ...ids]), error: null });
    const optimistic = mark(ids, () => null);
    if (optimistic) withDigest(optimistic);
    const result = await api.decideSuggestionsBySource({ shareId, decision });
    const busy = new Set(state.busy);
    for (const id of ids) busy.delete(id);
    if (!result.ok) {
      const error = toSuperDriveError(result);
      if (before) withDigest(before, { busy, error });
      else set({ busy, error });
      return { ok: false, error };
    }
    const written = result.value.results.reduce((sum, entry) => sum + (entry.written ?? 0), 0);
    const firstFailure = result.value.results.find((entry) => entry.status === "failed");
    set({ busy, lastOutcome: { kind: "source", decided: result.value.decided, failed: result.value.failed, written, ...(firstFailure?.code ? { code: firstFailure.code } : {}) } });
    await load(state.day ?? undefined);
    return { ok: true, value: { decided: result.value.decided, failed: result.value.failed } };
  }

  async function loadDiff(id: string): Promise<SuggestionActionResult<SuggestionDiff>> {
    const result = await api.suggestionDiff(id);
    return result.ok ? { ok: true, value: result.value } : { ok: false, error: toSuperDriveError(result) };
  }

  return Object.freeze({
    getState: () => state,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    load, decide, decideSource, loadDiff,
    clearOutcome: () => set({ lastOutcome: null }),
  });
}

export type SuggestionStore = ReturnType<typeof createSuggestionStore>;
