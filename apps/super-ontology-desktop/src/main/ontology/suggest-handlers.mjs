import { SuperOntologyError } from "../../shared/super-ontology/errors.mjs";
import { ensure } from "./response-guards.mjs";

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const isSuggestion = (value) => isObject(value) && typeof value.id === "string" && typeof value.share_id === "string"
  && typeof value.kind === "string" && typeof value.status === "string" && Number.isSafeInteger(value.revision);
const isDiff = (value) => isObject(value) && typeof value.suggestion_id === "string" && Array.isArray(value.changes) && typeof value.writable === "boolean";
const isDigest = (value) => isObject(value) && typeof value.day === "string" && Array.isArray(value.in_effect) && Array.isArray(value.pending);
/** The list endpoint answers `{ items }` without a cursor; the contract names a Page, so both are accepted. */
const isSuggestionList = (value) => isObject(value) && Array.isArray(value.items) && value.items.every(isSuggestion);

/** Most suggestions one "accept all from this source" walks. */
export const DECIDE_SOURCE_LIMIT = 500;

/**
 * Suggest channels (20-frontend §3.5.4, F-S7-01, F-S8-01, F-S8-03). Reading goes straight to the engine; accepting a suggestion
 * of an `editor` Share also writes the files (Desktop's job, 10 §11.1): check and plan first, accept at the engine, then
 * write; a failed write takes the acceptance back. Nothing is written for any other role.
 */
export function createSuggestHandlers({ client, writeback }) {
  const call = (name, options) => client.request(name, options);

  async function decide({ id, decision, revision, note }) {
    const body = { decision, ...(revision === undefined ? {} : { revision }), ...(note === undefined ? {} : { note }) };
    const { value } = await call("suggestionDecide", { params: { id }, body });
    return ensure(value, isSuggestion);
  }

  /** One decision, with the file write-back when an editor Share's suggestion is accepted. */
  async function decideWithWriteback(request) {
    if (request.decision !== "accept" || !writeback) return { suggestion: await decide(request), writeback: null };
    const { value: current } = await call("suggestionGet", { params: { id: request.id } });
    ensure(current, isSuggestion);
    const { value: diff } = await call("suggestionDiff", { params: { id: request.id } });
    ensure(diff, isDiff);
    if (diff.writable !== true) return { suggestion: await decide(request), writeback: null };
    const plan = await writeback.prepare({ suggestion: current, diff });
    const suggestion = await decide(request);
    try {
      return { suggestion, writeback: await writeback.commit(plan, { suggestion: current, diff }) };
    } catch (error) {
      // The files are back as they were (commit restores them); take the acceptance back so the suggestion can be reviewed again.
      await call("suggestionDecide", { params: { id: request.id }, body: { decision: "undo", note: "write-back failed" } }).catch(() => undefined);
      throw error;
    }
  }

  return {
    "suggestions-list": async ({ status, shareId, day, limit }) => {
      const { value } = await call("suggestionsList", { query: { status, share_id: shareId, day, limit } });
      ensure(value, isSuggestionList);
      return { items: value.items, next_cursor: typeof value.next_cursor === "string" ? value.next_cursor : null, as_of_seq: Number.isSafeInteger(value.as_of_seq) ? value.as_of_seq : 0 };
    },
    "suggestions-digest": async ({ day }) => {
      const { value } = await call("suggestionsDigest", { query: { day } });
      return ensure(value, isDigest);
    },
    "suggestion-diff": async ({ id }) => {
      const { value } = await call("suggestionDiff", { params: { id } });
      return ensure(value, isDiff);
    },
    "suggestion-decide": async ({ id, decision, revision, note }) => decideWithWriteback({ id, decision, revision, note }),

    /** "Accept (or reject) everything from this source": the pending suggestions of one Share, one decision each, in the order the engine lists them. */
    "suggestions-decide-source": async ({ shareId, decision }) => {
      const { value } = await call("suggestionsList", { query: { status: "pending", share_id: shareId, limit: DECIDE_SOURCE_LIMIT } });
      ensure(value, isSuggestionList);
      const results = [];
      for (const item of value.items.slice(0, DECIDE_SOURCE_LIMIT)) {
        try {
          const outcome = await decideWithWriteback({ id: item.id, decision, revision: item.revision });
          results.push({ id: item.id, status: outcome.suggestion.status, written: outcome.writeback?.written.length ?? 0 });
        } catch (error) {
          results.push({ id: item.id, status: "failed", code: error instanceof SuperOntologyError ? error.code : "SO_INTERNAL" });
        }
      }
      return { results, decided: results.filter((entry) => entry.status !== "failed").length, failed: results.filter((entry) => entry.status === "failed").length };
    },
  };
}
