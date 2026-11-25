import { describe, expect, it } from "vitest";
import type { StructureClass, StructureProperty } from "../../../../src/shared/super-ontology/docs-api.generated";
import { createLocaleFormatters } from "@open-ontology/design-system/localization";
import { anchorLabel, exampleText, formatAmount, formatDecimal, instanceColumns, isKnownKind, KNOWN_KINDS, relationTarget, shownValue, sortedClasses, valueThingLabel, valueThingName, visibleProperties } from "../../../../src/renderer/model/structureModel";

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

describe("typed values (docs-api 1.17)", () => {
  const en = createLocaleFormatters("en");
  const de = createLocaleFormatters("de");
  const zh = createLocaleFormatters("zh-Hans");
  it("formats an amount with the locale's grouping and keeps the currency code", () => {
    expect(shownValue({ amount: "6850000 CNY" }, en)).toEqual({ key: "amount", text: "6,850,000 CNY", raw: "6850000 CNY", original: null });
    expect(shownValue({ amount: "6850000 CNY" }, de)?.text).toBe("6.850.000 CNY");
    expect(shownValue({ amount: "120000" }, en)?.text).toBe("120,000");
    expect(formatAmount("1250.5 USD", en)).toBe("1,250.5 USD");
    expect(formatAmount("about 5k", en)).toBe("about 5k");
  });
  it("keeps the text as written when the value was read from words", () => {
    expect(shownValue({ amount: "6850000 CNY", original: "六百八十五万元" }, en)).toEqual({ key: "amount", text: "6,850,000 CNY", raw: "6850000 CNY", original: "六百八十五万元" });
    expect(shownValue({ ratio: "0.88", original: "百分之八十八" }, zh)).toMatchObject({ key: "ratio", text: "88%", original: "百分之八十八" });
    expect(shownValue({ date: "2027-03-16", original: "三月十六号" }, en)).toMatchObject({ key: "date", text: "Mar 16, 2027", original: "三月十六号" });
    // An original that says nothing the value does not is not repeated.
    expect(shownValue({ number: "12", original: "12" }, en)?.original).toBeNull();
    expect(shownValue({ text: "Atlas", original: "  " }, en)?.original).toBeNull();
  });
  it("shows a ratio as a percentage and a number with grouping", () => {
    expect(shownValue({ ratio: "0.88" }, en)?.text).toBe("88%");
    expect(shownValue({ ratio: "0.125" }, en)?.text).toBe("12.5%");
    expect(shownValue({ ratio: "0.07" }, en)?.text).toBe("7%");
    expect(shownValue({ ratio: "0.88" }, de)?.text).toBe("88\u00a0%");
    expect(shownValue({ number: "12345.67" }, en)?.text).toBe("12,345.67");
    expect(formatDecimal("123456789012345678901", en)).toBe("123,456,789,012,345,678,901");
  });
  it("shows a date in the locale and leaves anything that is not an ISO day or month as written", () => {
    expect(shownValue({ date: "2027-03-16" }, de)?.text).toBe("16.03.2027");
    expect(shownValue({ date: "2027-03" }, en)?.text).toBe("March 2027");
    expect(shownValue({ date: "Q3 2027" }, en)?.text).toBe("Q3 2027");
  });
  it("shows text and enum values as they are", () => {
    expect(shownValue({ text: "Canary first" }, en)).toEqual({ key: "text", text: "Canary first", raw: "Canary first", original: null });
    expect(shownValue({ enum: "Engineer" }, en)?.text).toBe("Engineer");
  });
  it("shows an unknown key as its raw text and never throws", () => {
    expect(shownValue({ duration: "PT2H" }, en)).toEqual({ key: null, text: "PT2H", raw: "PT2H", original: null });
    expect(shownValue({ amount: 42 } as Record<string, unknown>, en)?.text).toBe("42");
    expect(shownValue({ nested: { a: 1 } }, en)?.text).toBe('{"nested":{"a":1}}');
    expect(shownValue({ original: "三月" }, en)?.text).toBe("三月");
    expect(shownValue({}, en)).toBeNull();
    expect(shownValue(null, en)).toBeNull();
  });
  it("names an open value thing by its value", () => {
    expect(valueThingLabel({ type: "Thing", label: "Ratio" })).toBe("Ratio");
    expect(valueThingLabel({ type: "Thing", label: "budgets" })).toBeNull();
    expect(valueThingLabel({ type: "Person", label: "Amount" })).toBeNull();
    expect(valueThingName("Ratio", "88%", en)).toBe("88%");
    expect(valueThingName("Ratio", "0.88", en)).toBe("88%");
    expect(valueThingName("Amount", "6,850,000", en)).toBe("6,850,000");
    expect(valueThingName("Amount", "6850000", en)).toBe("6,850,000");
    expect(valueThingName("Date", "2027-03-16", en)).toBe("Mar 16, 2027");
  });
});

describe("cell anchors and instance columns", () => {
  it("writes the cell range of a row the way a spreadsheet does", () => {
    expect(anchorLabel({ kind: "cells", sheet: "Sheet1", range: "A5:D5" })).toBe("Sheet1!A5:D5");
    expect(anchorLabel({ kind: "cells", sheet: null, range: "A5:D5" })).toBe("A5:D5");
    expect(anchorLabel({ kind: "cells", range: "A2:F2" })).toBe("A2:F2");
    expect(anchorLabel({ kind: "cells", sheet: "预算", range: "B3:E3" })).toBe("'预算'!B3:E3");
    expect(anchorLabel({ kind: "cells", sheet: "Q1 budget", range: "A5:D5" })).toBe("'Q1 budget'!A5:D5");
    expect(anchorLabel({ kind: "cells", sheet: "Bob's", range: "A1:B1" })).toBe("'Bob''s'!A1:B1");
    expect(anchorLabel({ kind: "text", line_start: 3 })).toBeNull();
    expect(anchorLabel({ kind: "cells", range: "" })).toBeNull();
    expect(anchorLabel(null)).toBeNull();
  });
  it("makes a column of every confirmed value property and leaves entity-valued ones to the links", () => {
    const c = cls({ shape: "table", properties: [
      prop({ slug: "name", value_type: "person", status: "confirmed" }),
      prop({ slug: "budget", value_type: "amount", status: "confirmed" }),
      prop({ slug: "owner", value_type: "text", status: "confirmed", relation: { target: "people_table", target_kind: "class", resolved: 1, predicate: "about" } }),
      prop({ slug: "note", value_type: "text", status: "proposed" }),
      prop({ slug: "yield", value_type: "percent", status: "confirmed" }),
    ] });
    expect(instanceColumns(c).map((p) => p.slug)).toEqual(["budget", "yield"]);
  });
});
