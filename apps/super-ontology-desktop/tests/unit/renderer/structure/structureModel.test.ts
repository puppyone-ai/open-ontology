import { describe, expect, it } from "vitest";
import type { StructureClass, StructureProperty } from "../../../../src/shared/super-ontology/docs-api.generated";
import { exampleText, isKnownKind, KNOWN_KINDS, relationTarget, sortedClasses, visibleProperties } from "../../../../src/renderer/model/structureModel";

const prop = (over: Partial<StructureProperty> = {}): StructureProperty => ({ slug: "date", name: "Date", keys: ["date"], source: "frontmatter", value_type: "date", multi: false, files_with: 4, coverage: 1, fill: 1, required: true, examples: ["2026-01-01"], enum_values: [], relation: null, status: "proposed", ...over });
const cls = (over: Partial<StructureClass> = {}): StructureClass => ({ class_key: "meeting_minutes", label: "meeting_minutes", kind: "meeting_minutes", shape: "document", status: "proposed", version: 1, files: 4, rows: 4, properties: [prop()], sample_paths: [], signature: ["date"], ...over });

describe("the structure model", () => {
  it("knows the six kinds the engine names and treats anything else as a folder label", () => {
    expect(KNOWN_KINDS).toHaveLength(6);
    expect(isKnownKind("contract")).toBe(true);
    expect(isKnownKind("other")).toBe(false);
  });
  it("hides rejected properties and rejected classes", () => {
    expect(visibleProperties(cls({ properties: [prop(), prop({ slug: "x", status: "rejected" })] })).map((p) => p.slug)).toEqual(["date"]);
    expect(sortedClasses([cls({ class_key: "a", status: "rejected" }), cls({ class_key: "b" })]).map((c) => c.class_key)).toEqual(["b"]);
  });
  it("puts what a person still has to look at first and keeps the engine's order otherwise", () => {
    const out = sortedClasses([cls({ class_key: "a", status: "confirmed" }), cls({ class_key: "b" }), cls({ class_key: "c", status: "confirmed" }), cls({ class_key: "d" })]);
    expect(out.map((c) => c.class_key)).toEqual(["b", "d", "a", "c"]);
  });
  it("cuts examples to three and a readable length", () => {
    expect(exampleText(prop({ examples: ["a", "b", "c", "d"] }))).toBe("a · b · c");
    expect(exampleText(prop({ examples: ["x".repeat(40)] }))).toBe(`${"x".repeat(27)}…`);
  });
  it("names the class a relation points at, or the entity type", () => {
    const people = cls({ class_key: "people_table", label: "people_table", kind: "people_table" });
    const toClass = prop({ relation: { target: "people_table", target_kind: "class", resolved: 1, predicate: "mentions" } });
    expect(relationTarget(toClass, [people])).toEqual({ label: "people_table", kind: "class" });
    expect(relationTarget(prop({ relation: { target: "Person", target_kind: "entity_type", resolved: 0.5, predicate: "mentions" } }), [])).toEqual({ label: "Person", kind: "entity_type" });
    expect(relationTarget(prop(), [])).toBeNull();
  });
});
