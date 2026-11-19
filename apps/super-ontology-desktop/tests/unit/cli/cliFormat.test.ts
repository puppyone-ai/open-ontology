import { describe, expect, it } from "vitest";
import { diffLines, namesFrom, tokenizeYamlLine, toCsv, toJsonRows, toQueryTable, toYaml } from "../../../src/renderer/model/cliFormat";

describe("query tables", () => {
  const table = toQueryTable({ columns: [{ name: "id", type: "int" }, { name: "note" }], rows: [[1, "a,b"], [2, "=SUM(A1)"], [3, null]], returned_rows: 3, truncated: true, duration_ms: 7, warnings: [{ message: "careful" }] });
  it("reads the Gateway answer", () => {
    expect(table).toMatchObject({ returned: 3, truncated: true, durationMs: 7, warnings: ["careful"] });
    expect(toQueryTable({ rows: [] })).toBeNull();
  });
  it("writes CSV that quotes commas and neutralises formulas", () => {
    expect(toCsv(table!)).toBe("id,note\n1,\"a,b\"\n2,'=SUM(A1)\n3,\n");
  });
  it("writes JSON rows keyed by column", () => {
    expect(JSON.parse(toJsonRows(table!))[2]).toEqual({ id: 3, note: null });
  });
  it("finds names in several answer shapes", () => {
    expect(namesFrom({ tables: [{ name: "a.b" }, "c.d"] }).map((n) => n.name)).toEqual(["a.b", "c.d"]);
    expect(namesFrom({ items: [{ path: "x/y.yaml", snippet: "s" }] })).toEqual([{ name: "x/y.yaml", detail: "s" }]);
    expect(namesFrom("nope")).toEqual([]);
  });
});

describe("yaml helpers", () => {
  it("writes sorted block YAML with safe scalars", () => {
    expect(toYaml({ b: [1, { c: "x y" }], a: "yes", e: {}, n: null })).toBe('a: "yes"\nb:\n  - 1\n  -\n    c: "x y"\ne: {}\nn: null\n');
  });
  it("tokenizes a line without losing text", () => {
    for (const line of ["key: value", "  - name: \"x\"", "# note", "  n: 12 # c", "- plain", "    nested_key:"]) expect(tokenizeYamlLine(line).map((t) => t.text).join("")).toBe(line);
    expect(tokenizeYamlLine("a: 1").map((t) => t.kind)).toContain("number");
    expect(tokenizeYamlLine("# c")[1].kind).toBe("comment");
  });
  it("diffs by lines with both line numbers", () => {
    const diff = diffLines("a\nb\nc\n", "a\nx\nc\nd\n");
    expect(diff.map((d) => `${d.kind}:${d.text}`)).toEqual(["same:a", "del:b", "add:x", "same:c", "add:d"]);
    expect(diff[2]).toMatchObject({ left: null, right: 2 });
    expect(diffLines("same\n", "same\n").every((d) => d.kind === "same")).toBe(true);
  });
});
