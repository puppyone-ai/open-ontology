/** Pure helpers of the CLI parity screens: result tables, CSV/JSON, a small YAML writer and a line diff. No I/O. */
export type QueryColumn = { name: string; type?: string };
export type QueryTable = { columns: QueryColumn[]; rows: unknown[][]; returned: number; truncated: boolean; durationMs: number | null; warnings: string[] };

const isObject = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value);

export function toQueryTable(data: unknown): QueryTable | null {
  if (!isObject(data) || !Array.isArray(data.columns) || !Array.isArray(data.rows)) return null;
  const columns = data.columns.map((column) => (typeof column === "string" ? { name: column } : isObject(column) && typeof column.name === "string" ? { name: column.name, ...(typeof column.type === "string" ? { type: column.type } : {}) } : { name: "?" }));
  const rows = data.rows.filter(Array.isArray) as unknown[][];
  const warnings = Array.isArray(data.warnings) ? data.warnings.map((w) => (isObject(w) && typeof w.message === "string" ? w.message : String(isObject(w) ? w.code ?? "" : w))).filter(Boolean) : [];
  return {
    columns, rows, returned: typeof data.returned_rows === "number" ? data.returned_rows : rows.length, truncated: data.truncated === true,
    durationMs: typeof data.duration_ms === "number" ? data.duration_ms : null, warnings,
  };
}

const cell = (value: unknown): string => (value === null || value === undefined ? "" : typeof value === "object" ? JSON.stringify(value) : String(value));
const csvCell = (value: string): string => (/[",\r\n]/u.test(value) ? `"${value.replaceAll('"', '""')}"` : value);
/** Spreadsheet formula injection: a cell that starts with = + - @ is prefixed so it stays text when the CSV is opened. */
const safeCsv = (value: string): string => (/^[=+@]|^-(?![0-9.]+$)/u.test(value) ? `'${value}` : value);

export function toCsv(table: QueryTable): string {
  const lines = [table.columns.map((c) => csvCell(safeCsv(c.name))).join(",")];
  for (const row of table.rows) lines.push(table.columns.map((_, index) => csvCell(safeCsv(cell(row[index])))).join(","));
  return `${lines.join("\n")}\n`;
}

export function toJsonRows(table: QueryTable): string {
  return `${JSON.stringify(table.rows.map((row) => Object.fromEntries(table.columns.map((c, index) => [c.name, row[index] ?? null]))), null, 2)}\n`;
}

export const displayCell = cell;

/** Names from a `tables`/`knowledge search` style answer, whatever the exact field is called. */
export function namesFrom(data: unknown, keys: readonly string[] = ["tables", "items", "results", "hits", "entries"]): { name: string; detail: string | null }[] {
  const list = Array.isArray(data) ? data : isObject(data) ? keys.map((key) => data[key]).find(Array.isArray) : null;
  if (!Array.isArray(list)) return [];
  return list.map((item) => {
    if (typeof item === "string") return { name: item, detail: null };
    if (!isObject(item)) return null;
    const name = [item.path, item.table, item.name, item.id].find((v) => typeof v === "string");
    const detail = [item.snippet, item.description, item.summary, item.comment, item.text].find((v) => typeof v === "string");
    return typeof name === "string" ? { name, detail: typeof detail === "string" ? detail : null } : null;
  }).filter((item): item is { name: string; detail: string | null } => item !== null);
}

const PLAIN = /^[A-Za-z_][A-Za-z0-9_./-]*$/u;
const scalar = (value: unknown): string => {
  if (value === null || value === undefined) return "null";
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  const text = String(value);
  if (text === "") return '""';
  if (PLAIN.test(text) && !/^(true|false|null|yes|no|on|off|~)$/iu.test(text)) return text;
  return JSON.stringify(text);
};

/** A deterministic YAML 1.2 subset writer (block style; keys sorted unless `sort` is false) for ontology documents. */
export function toYaml(value: unknown, indent = 0, sort = true): string {
  const pad = "  ".repeat(indent);
  if (Array.isArray(value)) {
    if (value.length === 0) return `${pad}[]\n`;
    return value.map((item) => {
      if (isObject(item) || Array.isArray(item)) {
        const inner = toYaml(item, indent + 1, sort);
        return `${pad}-${inner.startsWith(`${"  ".repeat(indent + 1)}[]`) ? " []\n" : `\n${inner}`}`;
      }
      return `${pad}- ${scalar(item)}\n`;
    }).join("");
  }
  if (isObject(value)) {
    const keys = sort ? Object.keys(value).sort() : Object.keys(value);
    if (keys.length === 0) return `${pad}{}\n`;
    return keys.map((key) => {
      const item = value[key];
      const label = PLAIN.test(key) ? key : JSON.stringify(key);
      if ((isObject(item) && Object.keys(item).length > 0) || (Array.isArray(item) && item.length > 0)) return `${pad}${label}:\n${toYaml(item, indent + 1, sort)}`;
      if (typeof item === "string" && item.includes("\n")) return `${pad}${label}: |\n${item.split("\n").map((line) => `${pad}  ${line}`).join("\n")}\n`;
      return `${pad}${label}: ${isObject(item) ? "{}" : Array.isArray(item) ? "[]" : scalar(item)}\n`;
    }).join("");
  }
  return `${pad}${scalar(value)}\n`;
}

export type DiffLine = { kind: "same" | "add" | "del"; text: string; left: number | null; right: number | null };

/** Line diff by longest common subsequence (inputs are bounded to a few thousand lines; larger ones are compared as a whole). */
export function diffLines(a: string, b: string, limit = 4000): DiffLine[] {
  const left = a.split("\n"); const right = b.split("\n");
  if (left.at(-1) === "") left.pop();
  if (right.at(-1) === "") right.pop();
  if (left.length > limit || right.length > limit) return [...left.map((text, i) => ({ kind: "del" as const, text, left: i + 1, right: null })), ...right.map((text, i) => ({ kind: "add" as const, text, left: null, right: i + 1 }))];
  const n = left.length; const m = right.length;
  const table: Uint16Array[] = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i -= 1) for (let j = m - 1; j >= 0; j -= 1) table[i][j] = left[i] === right[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1]);
  const out: DiffLine[] = [];
  let i = 0; let j = 0;
  while (i < n && j < m) {
    if (left[i] === right[j]) { out.push({ kind: "same", text: left[i], left: i + 1, right: j + 1 }); i += 1; j += 1; }
    else if (table[i + 1][j] >= table[i][j + 1]) { out.push({ kind: "del", text: left[i], left: i + 1, right: null }); i += 1; }
    else { out.push({ kind: "add", text: right[j], left: null, right: j + 1 }); j += 1; }
  }
  for (; i < n; i += 1) out.push({ kind: "del", text: left[i], left: i + 1, right: null });
  for (; j < m; j += 1) out.push({ kind: "add", text: right[j], left: null, right: j + 1 });
  return out;
}

export type YamlToken = { text: string; kind: "key" | "string" | "number" | "keyword" | "comment" | "punct" | "plain" };

/** Highlight tokens of one YAML line (no HTML is built from file text; the screen renders spans). */
export function tokenizeYamlLine(line: string): YamlToken[] {
  const comment = /^(\s*)(#.*)$/u.exec(line);
  if (comment) return [{ text: comment[1], kind: "plain" }, { text: comment[2], kind: "comment" }];
  const match = /^(\s*(?:-\s+)?)((?:"[^"]*"|[A-Za-z0-9_./$-]+)\s*:)(\s.*|$)/u.exec(line);
  const out: YamlToken[] = [];
  let rest = line;
  if (match) { out.push({ text: match[1], kind: match[1].includes("-") ? "punct" : "plain" }, { text: match[2], kind: "key" }); rest = match[3]; }
  else { const dash = /^(\s*-\s+)(.*)$/u.exec(line); if (dash) { out.push({ text: dash[1], kind: "punct" }); rest = dash[2]; } else { const lead = /^\s*/u.exec(line)?.[0] ?? ""; out.push({ text: lead, kind: "plain" }); rest = line.slice(lead.length); } }
  const value = /^(\s*)(.*?)(\s+#.*)?$/u.exec(rest);
  if (value) {
    if (value[1]) out.push({ text: value[1], kind: "plain" });
    const body = value[2];
    if (body) out.push({ text: body, kind: /^".*"$|^'.*'$/u.test(body) ? "string" : /^-?[0-9][0-9_.eE+-]*$/u.test(body) ? "number" : /^(true|false|null|~)$/u.test(body) ? "keyword" : "plain" });
    if (value[3]) out.push({ text: value[3], kind: "comment" });
  } else out.push({ text: rest, kind: "plain" });
  return out.filter((token) => token.text !== "");
}
