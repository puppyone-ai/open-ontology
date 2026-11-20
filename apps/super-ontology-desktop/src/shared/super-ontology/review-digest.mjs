/**
 * The build review (D88): what the Builder proposed, reduced to what a person decides on. Pure functions over the two files the CLI
 * leaves in the stack's `.oo` folder when `oo build` stops for owner review (`adjudication.json`, `review-input.json`), and the
 * `oo.build-review/v1` document the app hands back with `oo build --resume --review-file`. The CLI recomputes and binds the summary
 * hash itself; nothing here is a second implementation of the review, only a reading of its inputs and a writer of its decision file.
 */
export const REVIEW_PROTOCOL = "oo.build-review/v1";
export const OVERRIDE_PROTOCOL = "oo.adjudication-override/v1";
const RELATION_DECISIONS = Object.freeze(["adopted", "rejected"]);
const text = (value) => (typeof value === "string" ? value : "");
const num = (value) => (typeof value === "number" && Number.isFinite(value) ? value : null);
const list = (value) => (Array.isArray(value) ? value : []);
const obj = (value) => (value && typeof value === "object" && !Array.isArray(value) ? value : {});
const bare = (hash) => text(hash).replace(/^sha256:/u, "");

/** A relation can multiply rows on a JOIN when its parent side is not unique (the CLI refuses to adopt it). */
export const fansOut = (item) => item?.class === "relation" && (obj(item.evidence).parent_unique === false || (num(obj(item.evidence).max_fanout) ?? 0) > 1);

function splitColumn(qualified) {
  const at = text(qualified).lastIndexOf(".");
  return at < 0 ? { table: text(qualified), column: "" } : { table: qualified.slice(0, at), column: qualified.slice(at + 1) };
}

/** `declared` (a foreign key the database states), `discovered` (found by comparing values), `named` (matched by column names or the model). */
export function relationOrigin(evidence) {
  const e = obj(evidence);
  if (e.source === "declared" || e.kind === "declared_fk") return "declared";
  if (e.source === "discovery") return "discovered";
  return "named";
}

const ORIGIN_ORDER = ["declared", "named", "discovered"];

/** @returns the digest the review screen shows, or null when the inputs are not a pending review. */
export function digestReview(adjudication, reviewInput) {
  const adj = obj(adjudication);
  if (adj.protocol !== "oo.adjudication/v1" || !Array.isArray(adj.items) || !Array.isArray(obj(adj.graph).entities)) return null;
  const input = obj(reviewInput);
  const items = adj.items.filter((item) => item && typeof item === "object" && typeof item.id === "string");

  const entities = list(adj.graph.entities).map((entity) => ({ id: text(entity.id), label: text(entity.label), table: text(entity.table), key: list(entity.key).map(text), display: entity.display === null || entity.display === undefined ? null : text(entity.display), rows: num(entity.rows) }));

  const datasets = list(obj(input.effective).datasets).map((dataset) => ({
    table: text(dataset.dataset),
    columns: list(dataset.columns).map((column) => ({ name: text(column.name), type: text(column.data_type), semantic: text(column.semantic_type), pii: text(column.pii) })),
  }));

  const groups = new Map();
  for (const item of items.filter((entry) => entry.class === "relation")) {
    const subject = obj(item.subject);
    const key = `${text(subject.from)}->${text(subject.to)}`;
    const evidence = obj(item.evidence);
    const from = splitColumn(subject.from);
    const to = splitColumn(subject.to);
    const existing = groups.get(key);
    if (existing) {
      existing.ids.push(item.id);
      if (item.decision === "adopted") existing.defaultDecision = "adopted";
      continue;
    }
    groups.set(key, {
      key, ids: [item.id], from: { table: from.table, columns: list(evidence.from_columns).length ? list(evidence.from_columns).map(text) : [from.column] }, to: { table: to.table, columns: list(evidence.to_columns).length ? list(evidence.to_columns).map(text) : [to.column] },
      cardinality: text(evidence.cardinality) || "many_to_one", origin: relationOrigin(evidence), defaultDecision: item.decision === "adopted" ? "adopted" : "rejected",
      rule: text(item.rule_id), reason: text(item.reason), confidence: num(obj(item.scores).confidence),
      evidence: { containment: num(evidence.containment), rowsChecked: num(evidence.rows_checked), matchedRows: num(evidence.matched_rows), unmatchedRows: num(evidence.unmatched_rows), nameSimilarity: num(evidence.name_similarity), parentUnique: evidence.parent_unique === true, maxFanout: num(evidence.max_fanout), childDistinct: num(evidence.child_distinct), parentRows: num(evidence.parent_rows) },
      fanOut: fansOut(item),
    });
  }
  const relations = [...groups.values()].sort((a, b) => ORIGIN_ORDER.indexOf(a.origin) - ORIGIN_ORDER.indexOf(b.origin) || a.key.localeCompare(b.key));

  const sensitive = items.filter((item) => item.class === "sensitive");
  const summary = obj(obj(obj(input.draft).review).summary);
  const discovery = obj(adj.discovery);
  return {
    summaryHash: text(adj.summary_sha256), inputHash: text(adj.input_sha256),
    entities, datasets, relations,
    counts: {
      tables: datasets.length || entities.length, columns: datasets.reduce((total, dataset) => total + dataset.columns.length, 0), entities: entities.length,
      declared: relations.filter((r) => r.origin === "declared").length, adopted: relations.filter((r) => r.defaultDecision === "adopted").length, discovered: relations.filter((r) => r.origin === "discovered").length,
      sensitiveHidden: sensitive.filter((item) => item.decision === "hidden").length, sensitiveChecked: sensitive.length,
    },
    discovery: { status: text(discovery.status) || "complete", checked: num(discovery.checked) ?? 0, remaining: num(discovery.remaining) ?? 0, unchecked: list(discovery.unchecked).length },
    builder: { declaredForeignKeys: num(summary.declared_fk_count), mappedForeignKeys: num(summary.mapped_fk_count), mappingRate: num(summary.fk_mapping_rate), namingConflicts: num(summary.naming_conflict_count), orphanRatio: num(summary.orphan_entity_ratio) },
  };
}

/**
 * The owner's decisions -> the `oo.build-review/v1` document. `decisions` maps a relation group key to `adopted` or `rejected`; only
 * groups whose decision differs from the Builder's proposal become overrides. Throws an Error whose message is a code on anything the CLI would refuse.
 */
export function buildReviewDocument(adjudication, decisions, reason) {
  const adj = obj(adjudication);
  const note = text(reason).trim();
  if (adj.protocol !== "oo.adjudication/v1" || note === "" || note.length > 1000) throw new Error("REVIEW_REASON_REQUIRED");
  const byKey = new Map();
  for (const item of list(adj.items).filter((entry) => entry.class === "relation")) {
    const key = `${text(obj(item.subject).from)}->${text(obj(item.subject).to)}`;
    byKey.set(key, [...(byKey.get(key) ?? []), item]);
  }
  const items = {};
  for (const [key, decision] of Object.entries(obj(decisions))) {
    const group = byKey.get(key);
    if (!group || !RELATION_DECISIONS.includes(decision)) throw new Error("REVIEW_DECISION_INVALID");
    for (const item of group) {
      if (item.decision === decision) continue;
      if (decision === "adopted" && fansOut(item)) throw new Error("REVIEW_FANOUT");
      items[item.id] = { decision, reason: note };
    }
  }
  const document = { protocol: REVIEW_PROTOCOL, reason: note };
  if (Object.keys(items).length > 0) document.overrides = { protocol: OVERRIDE_PROTOCOL, summary_sha256: bare(adj.summary_sha256), items };
  return document;
}
