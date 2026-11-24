import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { buildReviewDocument, digestReview, relationOrigin } from "../../../src/shared/super-ontology/review-digest.mjs";

// A real Builder draft for a synthetic four-table shop (4 tables, 3 foreign keys), recorded from `oo build`.
const recorded = path.resolve(import.meta.dirname, "../../support/recorded");
const adjudication = JSON.parse(fs.readFileSync(path.join(recorded, "adjudication.json"), "utf8"));
const input = JSON.parse(fs.readFileSync(path.join(recorded, "review-input.json"), "utf8"));
const digest = digestReview(adjudication, input);

describe("review digest", () => {
  it("reduces the draft to tables, entities and one row per relationship", () => {
    expect(digest.counts).toMatchObject({ tables: 4, columns: 14, entities: 4, declared: 3, discovered: 6, adopted: 3, sensitiveHidden: 0, sensitiveChecked: 14 });
    expect(digest.entities.map((entity) => entity.table).sort()).toEqual(["public.customers", "public.order_items", "public.orders", "public.products"]);
    expect(digest.entities.find((entity) => entity.table === "public.orders")).toMatchObject({ key: ["id"], rows: 60 });
    expect(digest.relations).toHaveLength(9);
  });
  it("groups duplicates of one foreign key and keeps the Builder's default", () => {
    const declared = digest.relations.filter((relation) => relation.origin === "declared");
    expect(declared.map((relation) => relation.key).sort()).toEqual(["public.order_items.order_id->public.orders.id", "public.order_items.product_id->public.products.id", "public.orders.customer_id->public.customers.id"]);
    expect(declared.every((relation) => relation.defaultDecision === "adopted" && relation.ids.length === 2 && !relation.fanOut)).toBe(true);
    const discovered = digest.relations.filter((relation) => relation.origin === "discovered");
    expect(discovered.every((relation) => relation.defaultDecision === "rejected" && relation.rule === "relation.no_signal")).toBe(true);
    expect(digest.relations.map((relation) => relation.origin)).toEqual([...digest.relations.map((relation) => relation.origin)].sort((a, b) => ["declared", "named", "discovered"].indexOf(a) - ["declared", "named", "discovered"].indexOf(b)));
  });
  it("carries the evidence a person decides on", () => {
    const fk = digest.relations.find((relation) => relation.key === "public.orders.customer_id->public.customers.id");
    expect(fk.evidence).toMatchObject({ containment: 1, rowsChecked: 60, matchedRows: 60, unmatchedRows: 0, parentUnique: true });
    expect(fk.from).toEqual({ table: "public.orders", columns: ["customer_id"] });
  });
  it("is not a review without the CLI's documents", () => {
    expect(digestReview(null, input)).toBeNull();
    expect(digestReview({ protocol: "x" }, input)).toBeNull();
    expect(relationOrigin({ source: "model" })).toBe("named");
  });
});

describe("review document", () => {
  const found = digest.relations.find((relation) => relation.origin === "discovered");
  const declared = digest.relations.find((relation) => relation.origin === "declared");
  it("is just the reason when nothing differs from the proposal", () => {
    expect(buildReviewDocument(adjudication, {}, "  Checked  ")).toEqual({ protocol: "oo.build-review/v1", reason: "Checked" });
    expect(buildReviewDocument(adjudication, { [found.key]: "rejected", [declared.key]: "adopted" }, "ok").overrides).toBeUndefined();
  });
  it("overrides only what changed, on every duplicate id, bound to the summary hash", () => {
    const document = buildReviewDocument(adjudication, { [found.key]: "adopted", [declared.key]: "rejected" }, "Reviewed");
    expect(document.overrides.summary_sha256).toBe(adjudication.summary_sha256.replace(/^sha256:/u, ""));
    expect(Object.keys(document.overrides.items).sort()).toEqual([...found.ids, ...declared.ids].sort());
    expect(Object.values(document.overrides.items).filter((item) => item.decision === "rejected")).toHaveLength(declared.ids.length);
    expect(Object.values(document.overrides.items).every((item) => item.reason === "Reviewed")).toBe(true);
  });
  it("refuses an empty reason, an unknown relationship and a fan-out", () => {
    expect(() => buildReviewDocument(adjudication, {}, "   ")).toThrow("REVIEW_REASON_REQUIRED");
    expect(() => buildReviewDocument(adjudication, { "a.b->c.d": "adopted" }, "x")).toThrow("REVIEW_DECISION_INVALID");
    expect(() => buildReviewDocument(adjudication, { [found.key]: "maybe" }, "x")).toThrow("REVIEW_DECISION_INVALID");
    const fanOut = structuredClone(adjudication);
    for (const item of fanOut.items) if (item.id === found.ids[0]) item.evidence.max_fanout = 3;
    expect(() => buildReviewDocument(fanOut, { [found.key]: "adopted" }, "x")).toThrow("REVIEW_FANOUT");
  });
});
