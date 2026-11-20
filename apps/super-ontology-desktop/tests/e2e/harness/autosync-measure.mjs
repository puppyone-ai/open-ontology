// Measuring how fast a change outside (or inside) the app reaches the ontology (P07). Pure: a `probe` supplies what the engine and the page show,
// `now`/`sleep` supply the clock, so the logic is tested without an app. Budgets come from decision D49 and the watcher's 30 s safety poll.

/** Per-file budgets in ms. md/txt: D49 (p95 7 s after the L3 calibration); a row must show well before the engine's 30 s poll could have found it. */
export const BUDGET = Object.freeze({
  rowMs: 5_000,
  publishMs: 7_000,
  unsupportedMs: 7_000,
  goneMs: 7_000,
  settleMs: 9_000,
  uiLagMs: 3_000,
  bulkPublishMs: 60_000,
  bulkRowMs: 15_000,
  ringLagMs: 3_000,
  pollIntervalMs: 30_000,
});

const GONE_STATES = new Set(["deleted", "superseded", "duplicate"]);

/** The freshness states a trace has gone through, consecutive duplicates dropped. */
export class Trace {
  constructor() { this.entries = []; }
  add(ms, state) {
    if (typeof state !== "string") return;
    if (this.entries.at(-1)?.state !== state) this.entries.push({ ms, state });
  }
  get last() { return this.entries.at(-1)?.state ?? null; }
  get left() { return this.entries.some((entry) => entry.state !== "up_to_date"); }
  states() { return this.entries.map((entry) => entry.state); }
}

/**
 * `expect`: [{ path, want, changedFrom? }] with want `published`, `unsupported` or `gone`; `changedFrom` (a b3) is for an edit: the engine has
 * seen the change once the file's b3 differs, and it is settled once it is `want` with a different b3. `probe()` answers
 * { files: Map<path,{ state, b3 }>, freshness: string|null, uiFreshness: string|null, uiRows: Map<name,state>|null, lastSync: object|null }.
 * Polls until every expectation holds and the freshness (engine and page) is up_to_date again, or `timeoutMs` passes.
 */
export async function measureChange({ expect, probe, now = Date.now, sleep, intervalMs = 100, timeoutMs = 90_000, uiRequired = false, quietSamples = 2, offsetMs = 0 }) {
  const t0 = now();
  const per = new Map(expect.map((entry) => [entry.path, { path: entry.path, want: entry.want, changedFrom: entry.changedFrom, rowMs: null, settledMs: null, uiRowMs: null, uiSettledMs: null, finalState: null }]));
  const api = new Trace();
  const ui = new Trace();
  let quiet = 0;
  let freshMs = null;
  const syncs = new Map();
  let timedOut = false;
  for (;;) {
    const sample = await probe();
    const t = now() - t0 + offsetMs;
    api.add(t, sample.freshness);
    ui.add(t, sample.uiFreshness);
    if (sample.lastSync) syncs.set(`${sample.lastSync.generation}|${sample.lastSync.finished_at}`, sample.lastSync);
    for (const item of per.values()) {
      const found = sample.files.get(item.path);
      const state = found?.state;
      const stale = item.changedFrom !== undefined && found?.b3 === item.changedFrom;
      item.finalState = state ?? null;
      const present = state !== undefined && !GONE_STATES.has(state);
      if (item.want === "gone") {
        if (!present && item.settledMs === null) item.settledMs = t;
      } else {
        if (present && !stale && item.rowMs === null) item.rowMs = t;
        if (state === item.want && !stale && item.settledMs === null) item.settledMs = t;
      }
      if (sample.uiRows) {
        const name = item.path.split("/").at(-1);
        const shown = sample.uiRows.get(name);
        if (item.want === "gone") { if (shown === undefined && item.uiSettledMs === null) item.uiSettledMs = t; }
        else {
          if (shown !== undefined && item.uiRowMs === null) item.uiRowMs = t;
          if (shown === item.want && item.uiSettledMs === null) item.uiSettledMs = t;
        }
      }
    }
    const allSettled = [...per.values()].every((item) => item.settledMs !== null && (!uiRequired || item.uiSettledMs !== null));
    const fresh = sample.freshness === "up_to_date" && (sample.uiFreshness === null || sample.uiFreshness === undefined || sample.uiFreshness === "up_to_date");
    if (allSettled && fresh) { quiet += 1; if (quiet >= quietSamples) { freshMs = t; break; } } else quiet = 0;
    if (t > timeoutMs) { timedOut = true; break; }
    await sleep(intervalMs);
  }
  return { totalMs: now() - t0, freshMs, timedOut, files: [...per.values()], freshness: { api: api.entries, ui: ui.entries, apiLeftUpToDate: api.left, apiEnd: api.last, uiEnd: ui.last }, syncs: [...syncs.values()] };
}

/** Problems of one measurement against the budgets (empty = pass). Nothing here is relaxed to make a run pass: a failure is the finding. */
export function judge(measurement, { rowMs = BUDGET.rowMs, settleMs = BUDGET.publishMs, uiRequired = false, expectTransition = true, notPoll = true, uiLagMs = BUDGET.uiLagMs, settledBudgetFor } = {}) {
  const problems = [];
  if (measurement.timedOut) problems.push(`did not settle within the wait (freshness ${measurement.freshness.apiEnd}, page ${measurement.freshness.uiEnd})`);
  for (const item of measurement.files) {
    const budget = settledBudgetFor?.(item) ?? (item.want === "gone" ? BUDGET.goneMs : settleMs);
    if (item.want !== "gone" && (item.rowMs === null || item.rowMs > rowMs)) problems.push(`${item.path}: row ${item.rowMs === null ? "never appeared" : `${item.rowMs} ms`} > ${rowMs} ms`);
    if (item.settledMs === null || item.settledMs > budget) problems.push(`${item.path}: ${item.want} ${item.settledMs === null ? `never (last state ${item.finalState})` : `${item.settledMs} ms`} > ${budget} ms`);
    if (uiRequired) {
      if (item.uiSettledMs === null) problems.push(`${item.path}: the page never showed it ${item.want}`);
      else if (item.settledMs !== null && item.uiSettledMs - item.settledMs > uiLagMs) problems.push(`${item.path}: the page lagged the engine by ${item.uiSettledMs - item.settledMs} ms > ${uiLagMs} ms`);
    }
  }
  if (measurement.freshness.apiEnd !== "up_to_date") problems.push(`freshness ended ${measurement.freshness.apiEnd}, not up_to_date`);
  if (uiRequired && measurement.freshness.uiEnd !== "up_to_date") problems.push(`the page's freshness ended ${measurement.freshness.uiEnd}, not up_to_date`);
  if (expectTransition && !measurement.freshness.apiLeftUpToDate) problems.push("freshness never left up_to_date (it never showed the change being processed)");
  const byPoll = measurement.syncs.filter((sync) => sync.trigger === "poll" && (sync.added ?? 0) + (sync.changed ?? 0) + (sync.removed ?? 0) + (sync.renamed ?? 0) > 0);
  if (notPoll && byPoll.length > 0) problems.push(`the sync that took the change was the ${BUDGET.pollIntervalMs / 1000} s poll (trigger poll), not the watcher`);
  return problems;
}

export function percentile(values, p) {
  const sorted = values.filter((value) => typeof value === "number").sort((a, b) => a - b);
  if (sorted.length === 0) return null;
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1))];
}
