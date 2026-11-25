import type { StructureClass, StructureProperty } from "../../shared/super-ontology/docs-api.generated";
import { invoke, type ApiResult } from "./bridgeInvoke";
import type { StructureInstances, StructureSchema, StructureYaml } from "../../shared/super-ontology/docs-api.generated";

/** The structure layer of the ontology (D93): the classes of record files, like the table schemas of a database. */
export const structureApi = {
  read: (driveId: string): Promise<ApiResult<StructureSchema>> => invoke("structure-read", { driveId }),
  induce: (driveId: string): Promise<ApiResult<StructureSchema>> => invoke("structure-induce", { driveId }),
  decide: (input: { driveId: string; op: "accept" | "reject" | "rename" | "merge"; classKey: string; property?: string | null; name?: string; into?: string }): Promise<ApiResult<StructureSchema>> => invoke("structure-decide", input),
  yaml: (driveId: string, database?: boolean): Promise<ApiResult<StructureYaml>> => invoke("structure-yaml", database === undefined ? { driveId } : { driveId, database }),
  /** One page of the instances of a confirmed table class (docs-api 1.17). */
  instances: (input: { driveId: string; classKey: string; limit?: number; cursor?: string }): Promise<ApiResult<StructureInstances>> => invoke("structure-instances", input),
};

export const KNOWN_KINDS = ["meeting_minutes", "decision_log", "weekly_report", "people_table", "project_table", "contract"] as const;

/** The class name the person sees: a known kind in their language, any other class by the label the engine found (its folder). */
export function isKnownKind(kind: string): kind is (typeof KNOWN_KINDS)[number] {
  return (KNOWN_KINDS as readonly string[]).includes(kind);
}

/** Properties that are not rejected, in the order the engine gives them (most common first). */
export function visibleProperties(c: StructureClass): StructureProperty[] {
  return c.properties.filter((p) => p.status !== "rejected");
}

/** Classes a person still has to look at come first; confirmed ones keep the engine's order. */
export function sortedClasses(classes: StructureClass[]): StructureClass[] {
  const rank = (c: StructureClass) => (c.status === "proposed" ? 0 : c.status === "confirmed" ? 1 : 2);
  return classes.filter((c) => c.status !== "rejected").map((c, i) => ({ c, i })).sort((a, b) => rank(a.c) - rank(b.c) || a.i - b.i).map((x) => x.c);
}

/** Up to three example values, cut to a readable length. */
export function exampleText(p: StructureProperty): string {
  return p.examples.slice(0, 3).map((e) => (e.length > 28 ? `${e.slice(0, 27)}…` : e)).join(" · ");
}

/** A relation names the class or entity type the values point at; `null` when the property is a plain value. */
export function relationTarget(p: StructureProperty, classes: StructureClass[]): { label: string; kind: "class" | "entity_type" } | null {
  const r = p.relation;
  if (!r) return null;
  const target = r.target_kind === "class" ? classes.find((c) => c.class_key === r.target) : undefined;
  return { label: target ? target.label : r.target, kind: r.target_kind === "class" ? "class" : "entity_type" };
}

/** What the value helpers need from the localization layer (`useLocalization()` gives both). */
export type ValueFormatters = {
  formatNumber: (value: number | bigint, options?: Intl.NumberFormatOptions) => string;
  formatDate: (value: Date | number | string, options?: Intl.DateTimeFormatOptions) => string;
};

/** The typed keys of an attribute value (docs-api 1.17), in the order they are looked for. */
export const VALUE_KEYS = ["amount", "ratio", "date", "number", "enum", "text"] as const;
export type ValueKey = (typeof VALUE_KEYS)[number];

/** An attribute value as a person reads it: the normalised value formatted for the locale and, when it was read from words, the text as written. */
export type ShownValue = { key: ValueKey | null; text: string; raw: string; original: string | null };

const PLAIN_NUMBER = /^-?\d+(?:\.\d+)?$/u;

/** A plain decimal (`6850000`, `12.5`) with the locale's grouping; very long integers keep every digit, anything else stays as written. */
export function formatDecimal(raw: string, f: ValueFormatters): string {
  if (!PLAIN_NUMBER.test(raw)) return raw;
  const [whole, fraction = ""] = raw.replace(/^-/u, "").split(".");
  if (whole.length + fraction.length <= 15) return f.formatNumber(Number(raw), { maximumFractionDigits: fraction.length });
  return fraction === "" ? f.formatNumber(BigInt(raw)) : raw;
}

/** `6850000 CNY` -> `6,850,000 CNY`; an amount without a currency is the number alone. */
export function formatAmount(raw: string, f: ValueFormatters): string {
  const match = /^(-?\d+(?:\.\d+)?)(?:\s+([A-Z]{3}))?$/u.exec(raw.trim());
  if (!match) return raw;
  return match[2] ? `${formatDecimal(match[1], f)} ${match[2]}` : formatDecimal(match[1], f);
}

/** A ratio is a fraction: `0.88` -> `88%`, `0.125` -> `12.5%` (in the locale's percent form). */
export function formatRatio(raw: string, f: ValueFormatters): string {
  if (!PLAIN_NUMBER.test(raw.trim())) return raw;
  const n = Number(raw);
  return Number.isFinite(n) ? f.formatNumber(n, { style: "percent", maximumFractionDigits: 2 }) : raw;
}

/** An ISO day (`2027-03-16`) or month (`2027-03`) in the locale; read as UTC so the day never moves. Anything else stays as written. */
export function formatIsoDate(raw: string, f: ValueFormatters): string {
  const value = raw.trim();
  if (/^\d{4}-\d{2}-\d{2}$/u.test(value) && !Number.isNaN(Date.parse(`${value}T00:00:00Z`))) return f.formatDate(`${value}T00:00:00Z`, { dateStyle: "medium", timeZone: "UTC" });
  if (/^\d{4}-\d{2}$/u.test(value) && !Number.isNaN(Date.parse(`${value}-01T00:00:00Z`))) return f.formatDate(`${value}-01T00:00:00Z`, { year: "numeric", month: "long", timeZone: "UTC" });
  return raw;
}

function scalar(entry: unknown): string | null {
  if (typeof entry === "string") return entry;
  if (typeof entry === "number" && Number.isFinite(entry)) return String(entry);
  return null;
}

/**
 * An attribute value (`{"amount":"6850000 CNY","original":"六百八十五万元"}`) as text: the typed key formatted for the locale, `original` kept when
 * it says something the value does not. A key this app does not know shows its raw text; `null` when there is no value at all.
 */
export function shownValue(value: Record<string, unknown> | null | undefined, f: ValueFormatters): ShownValue | null {
  if (!value || typeof value !== "object") return null;
  const original = typeof value.original === "string" && value.original.trim() !== "" ? value.original.trim() : null;
  for (const key of VALUE_KEYS) {
    const raw = scalar(value[key]);
    if (raw === null) continue;
    const text = key === "amount" ? formatAmount(raw, f) : key === "ratio" ? formatRatio(raw, f) : key === "date" ? formatIsoDate(raw, f) : key === "number" ? formatDecimal(raw, f) : raw;
    return { key, text, raw, original: original !== null && original !== raw && original !== text ? original : null };
  }
  const other = Object.entries(value).find(([key, entry]) => key !== "original" && scalar(entry) !== null);
  if (other) return { key: null, text: scalar(other[1]) as string, raw: scalar(other[1]) as string, original };
  const rest = Object.fromEntries(Object.entries(value).filter(([key]) => key !== "original"));
  if (Object.keys(rest).length === 0) return original === null ? null : { key: null, text: original, raw: original, original: null };
  const json = JSON.stringify(rest);
  return { key: null, text: json, raw: json, original };
}

/** The open value things that tie documents through a value (`Thing` labelled `Amount`, `Date`, `Ratio`), with the structure type that names them. */
export const VALUE_THING_TYPES = { Amount: "amount", Date: "date", Ratio: "percent" } as const;
export type ValueThingLabel = keyof typeof VALUE_THING_TYPES;

export function valueThingLabel(entity: { type: string; label?: string | null }): ValueThingLabel | null {
  return entity.type === "Thing" && typeof entity.label === "string" && Object.hasOwn(VALUE_THING_TYPES, entity.label) ? (entity.label as ValueThingLabel) : null;
}

/** The name of a value thing as a person reads it: a ratio is a percentage (`88%`), a date is in the locale, an amount keeps its grouping. */
export function valueThingName(label: ValueThingLabel, name: string, f: ValueFormatters): string {
  const value = name.trim();
  if (label === "Ratio") return /%$/u.test(value) ? value : formatRatio(value, f);
  if (label === "Date") return formatIsoDate(value, f);
  return PLAIN_NUMBER.test(value) ? formatDecimal(value, f) : formatAmount(value, f);
}

/** `Sheet1!A5:D5` for a cell range of a workbook, `A5:D5` for a CSV (no sheet); `null` for an anchor that is not a cell range. */
export function anchorLabel(anchor: Record<string, unknown> | null | undefined): string | null {
  if (!anchor || anchor.kind !== "cells" || typeof anchor.range !== "string" || anchor.range.trim() === "") return null;
  const range = anchor.range.trim();
  const sheet = typeof anchor.sheet === "string" ? anchor.sheet.trim() : "";
  if (sheet === "") return range;
  // Quoted like the engine's source marks and spreadsheets: anything but ASCII letters, digits and `_` (`'Q1 budget'!A5:D5`, `'预算'!B3:E3`).
  return /^[A-Za-z0-9_]+$/u.test(sheet) ? `${sheet}!${range}` : `'${sheet.replace(/'/gu, "''")}'!${range}`;
}

/** Entity-valued properties (a person, a project, an organization) are relations of the row, not values; the instance table shows them as links. */
const ENTITY_VALUE_TYPES = new Set(["person", "project", "organization"]);

/** The value columns of a filled table class: its confirmed properties that hold a value, in the engine's order. */
export function instanceColumns(c: StructureClass): StructureProperty[] {
  return c.properties.filter((p) => p.status === "confirmed" && !p.relation && !ENTITY_VALUE_TYPES.has(p.value_type));
}

/** Rows read per file per extraction (docs-api 1.17); a file with more counts in `truncated_files`. */
export const TABLE_ROW_LIMIT = 5000;
/** Instances asked for per page. */
export const INSTANCE_PAGE = 50;
