/**
 * The read-only tools of the Ask loop. Each one is a GET (or the one read-only SELECT) on the engine's HTTP surface with the app's
 * internal Agent token (`docs:read`, plus `db:read` when a database is connected): ontology search, entity, schema and query
 * (`/v1/ontology/*`, the same surface as `oo ontology ...`), full-text search over facts and blocks, and the blocks of one file.
 * A tool never writes, never takes a path on this computer and never returns a credential. What a tool prints for the model carries a
 * `[S<n>]` tag per source; only tagged sources can be cited in an answer, so a citation is always something a tool really returned.
 */
const RESULT_LIMIT = 6000;
const MAX_ROWS_SHOWN = 40;

export const TOOL_DEFINITIONS = Object.freeze([
  { name: "search", description: "Search the ontology (documents and database) by name or text. Matching is literal: try other spellings, other languages and shorter words when nothing comes back.", parameters: { type: "object", properties: { query: { type: "string" }, limit: { type: "integer", minimum: 1, maximum: 20 } }, required: ["query"] } },
  { name: "entity", description: "One entity (or table row) by its URN from a search result: its facts with their sources, and its links.", parameters: { type: "object", properties: { urn: { type: "string" } }, required: ["urn"] } },
  { name: "find_text", description: "Full-text search over facts and the text of files (every word must match; words are prefixes). Returns the matching passage and the file it is in.", parameters: { type: "object", properties: { query: { type: "string" }, limit: { type: "integer", minimum: 1, maximum: 20 } }, required: ["query"] } },
  { name: "read_file", description: "The text of one file, block by block, for a file_id from find_text or a drive_id and path from a search result. Use it when a passage needs its context.", parameters: { type: "object", properties: { file_id: { type: "string" }, drive_id: { type: "string" }, path: { type: "string" } } } },
  { name: "schema", description: "The tables, columns and relationships of the connected database.", parameters: { type: "object", properties: {} } },
  { name: "sql", description: "One read-only SELECT against the connected database (rows are capped).", parameters: { type: "object", properties: { sql: { type: "string" }, max_rows: { type: "integer", minimum: 1, maximum: 200 } }, required: ["sql"] } },
]);

/** What the quiet progress line says: the tool and its main argument, shown by the renderer in the person's language. */
export function describeCall(name, args) {
  const arg = name === "search" || name === "find_text" ? args.query : name === "entity" ? args.urn : name === "sql" ? args.sql : name === "read_file" ? (args.path ?? args.file_id) : "";
  return { tool: name, arg: typeof arg === "string" ? arg.slice(0, 160) : "" };
}

export function createSourceRegistry() {
  const byKey = new Map();
  const list = [];
  return {
    add(ref) {
      const key = ref.kind === "file" ? `f|${ref.fileId ?? ref.driveId}|${ref.path}|${ref.anchor ?? ""}`
        : ref.kind === "row" ? `r|${ref.source}|${ref.table}|${ref.pk}` : `q|${ref.queryId ?? ref.sql}`;
      let found = byKey.get(key);
      if (!found) { found = { id: `S${list.length + 1}`, ...ref }; byKey.set(key, found); list.push(found); }
      return found.id;
    },
    get: (id) => list.find((s) => s.id === id) ?? null,
    all: () => list.slice(),
  };
}

const str = (v) => (typeof v === "string" ? v : v === null || v === undefined ? "" : String(v));
const clip = (text, n = RESULT_LIMIT) => (text.length > n ? `${text.slice(0, n)}\n[cut at ${n} characters; narrow the request]` : text);

/** An anchor object of the engine as a short place name: a heading, lines, a page, a sheet cell. */
export function anchorText(anchor) {
  if (typeof anchor === "string") return anchor;
  if (!anchor || typeof anchor !== "object") return "";
  if (typeof anchor.heading === "string") return anchor.heading;
  if (Number.isInteger(anchor.line_start)) return anchor.line_end && anchor.line_end !== anchor.line_start ? `L${anchor.line_start}-L${anchor.line_end}` : `L${anchor.line_start}`;
  if (Number.isInteger(anchor.page)) return `p.${anchor.page}`;
  if (typeof anchor.cell === "string") return anchor.sheet ? `${anchor.sheet}!${anchor.cell}` : anchor.cell;
  if (typeof anchor.range === "string") return anchor.range;
  if (typeof anchor.start_ms === "number") return `${Math.round(anchor.start_ms / 1000)}s`;
  return "";
}

function refOfProvenance(p) {
  if (!p || typeof p !== "object") return null;
  if (p.kind === "file" && typeof p.path === "string") return { kind: "file", fileId: typeof p.file_id === "string" ? p.file_id : null, driveId: str(p.drive_id) || null, path: p.path, anchor: anchorText(p.anchor), blockId: str(p.block_id) };
  if (p.kind === "table_row") return { kind: "row", source: str(p.source), table: str(p.table), pk: str(p.pk), keyColumn: str(p.key_column) || "pk" };
  return null;
}

const where = (id, ref) => (ref.kind === "file" ? `${id} ${ref.path}${ref.anchor ? ` #${ref.anchor}` : ""}${ref.fileId ? ` (file_id ${ref.fileId})` : ref.driveId ? ` (drive_id ${ref.driveId})` : ""}` : `${id} table ${ref.table} row ${ref.keyColumn}=${ref.pk}`);

/**
 * @param {{ http: (method: string, path: string, body?: object, signal?: AbortSignal) => Promise<{ status: number, json: any }>, registry: ReturnType<typeof createSourceRegistry> }} deps
 */
export function createAskTools({ http, registry }) {
  const failure = (res) => {
    const e = res.json?.error ?? res.json ?? {};
    return `error ${res.status}${e.code ? ` ${e.code}` : ""}${e.message ? `: ${str(e.message).slice(0, 400)}` : ""}${e.hint ? ` (${str(e.hint).slice(0, 200)})` : ""}`;
  };

  const impl = {
    async search({ query, limit = 8 }, signal) {
      const res = await http("GET", `/v1/ontology/search?q=${encodeURIComponent(query)}&limit=${Math.min(20, Math.max(1, Number(limit) || 8))}`, undefined, signal);
      if (res.status !== 200) return failure(res);
      const lines = [];
      for (const item of res.json.items ?? []) {
        const ref = refOfProvenance(item.provenance);
        const tag = ref ? `[${registry.add(ref)}] ` : "";
        lines.push(`${tag}${str(item.side)} ${str(item.type)} ${str(item.name)}  urn=${str(item.urn)}${ref?.kind === "file" ? `  file ${ref.path}${ref.driveId ? ` drive_id ${ref.driveId}` : ""}` : ""}`);
        if (item.text) lines.push(`  text: ${str(item.text).slice(0, 300)}`);
        if (item.predicate) lines.push(`  predicate: ${str(item.predicate)}`);
      }
      for (const w of res.json.warnings ?? []) lines.push(`note: ${str(w)}`);
      if ((res.json.items ?? []).length === 0 && !(res.json.warnings ?? []).length) lines.push(`no results for "${query}". Search is literal: try another spelling, the other language's form of the word, or a shorter word.`);
      return clip(lines.join("\n"));
    },
    async entity({ urn }, signal) {
      const res = await http("GET", `/v1/ontology/entities/${encodeURIComponent(urn)}`, undefined, signal);
      if (res.status !== 200) return failure(res);
      const v = res.json;
      const lines = [`${str(v.urn)} ${str(v.name)} (${str(v.side)})`];
      const own = refOfProvenance(v.provenance);
      if (own) { const id = registry.add(own); lines.push(`  ${where(id, own)}`); }
      if (v.row) lines.push(`  row ${JSON.stringify(v.row).slice(0, 600)}`);
      for (const f of v.facts ?? []) {
        const ref = refOfProvenance(f.provenance);
        const tag = ref ? ` [${registry.add(ref)}]` : "";
        const value = typeof f.value === "string" ? f.value : f.object?.name ?? (f.value === undefined ? "" : JSON.stringify(f.value));
        lines.push(`  ${str(f.predicate)} = ${str(value).slice(0, 300)}${tag}`);
      }
      for (const l of v.links ?? []) {
        const rref = refOfProvenance(l.row?.provenance);
        lines.push(`  link -> ${str(l.target?.urn)}${l.row ? ` row ${JSON.stringify(l.row.columns).slice(0, 300)}${rref ? ` [${registry.add(rref)}]` : ""}` : ""}`);
      }
      return clip(lines.join("\n"));
    },
    async find_text({ query, limit = 8 }, signal) {
      const res = await http("GET", `/v1/docs/search?q=${encodeURIComponent(query)}&limit=${Math.min(20, Math.max(1, Number(limit) || 8))}`, undefined, signal);
      if (res.status !== 200) return failure(res);
      const lines = [];
      for (const hit of res.json.items ?? []) {
        const e = hit.evidence?.[0];
        const ref = e ? { kind: "file", fileId: str(e.file_id) || null, driveId: str(e.drive_id) || null, path: str(e.path), anchor: anchorText(e.anchor), blockId: str(e.block_id), quote: str(hit.text).slice(0, 240) } : null;
        const tag = ref ? `[${registry.add(ref)}] ` : "";
        lines.push(`${tag}${str(hit.kind)}${hit.subject?.name ? ` ${str(hit.subject.name)}` : ""}${hit.predicate ? ` ${str(hit.predicate)}` : ""}: ${str(hit.text).slice(0, 300)}${ref ? `  (${ref.path}${ref.anchor ? ` #${ref.anchor}` : ""}, file_id ${ref.fileId})` : ""}`);
      }
      if (lines.length === 0) lines.push(`no passage matches "${query}" (all words must match; words are prefixes). Try fewer or other words, or the other language's form.`);
      if (res.json.truncated) lines.push("note: more hits exist; narrow the query.");
      return clip(lines.join("\n"));
    },
    async read_file({ file_id: fileId, drive_id: driveId, path }, signal) {
      let id = typeof fileId === "string" && fileId ? fileId : null;
      let filePath = typeof path === "string" ? path : "";
      if (!id && driveId && path) {
        const list = await http("GET", `/v1/docs/drives/${encodeURIComponent(driveId)}/files?q=${encodeURIComponent(path.split("/").pop())}&recursive=true&limit=50`, undefined, signal);
        if (list.status !== 200) return failure(list);
        id = (list.json.items ?? []).find((f) => f.path === path)?.id ?? null;
        if (!id) return `no file at ${path} in that drive`;
      }
      if (!id) return "error: give a file_id, or a drive_id and a path";
      const detail = await http("GET", `/v1/docs/files/${encodeURIComponent(id)}`, undefined, signal);
      if (detail.status !== 200) return failure(detail);
      filePath = str(detail.json.path) || filePath;
      const res = await http("GET", `/v1/docs/files/${encodeURIComponent(id)}/blocks?limit=60`, undefined, signal);
      if (res.status !== 200) return failure(res);
      const out = [`${filePath}`];
      let size = 0;
      for (const block of res.json.items ?? []) {
        const text = str(block.text ?? block.content ?? "").replace(/\s+/gu, " ").trim();
        if (!text) continue;
        const anchor = anchorText(block.anchor) || str(block.block_id ?? block.id);
        const tag = registry.add({ kind: "file", fileId: id, driveId: str(detail.json.drive_id) || null, path: filePath, anchor, blockId: str(block.block_id ?? block.id), quote: text.slice(0, 240) });
        out.push(`[${tag}] ${anchor ? `#${anchor} ` : ""}${text.slice(0, 600)}`);
        size += text.length;
        if (size > RESULT_LIMIT - 800) { out.push("[cut: more blocks exist]"); break; }
      }
      if (out.length === 1) out.push("(this file has no readable text)");
      return clip(out.join("\n"));
    },
    async schema(_args, signal) {
      const res = await http("GET", "/v1/ontology/schema?limit=60", undefined, signal);
      if (res.status !== 200) return failure(res);
      const lines = [];
      for (const t of res.json.tables ?? []) lines.push(`${str(t.qualified_name)}: ${(t.columns ?? []).map((c) => `${str(c.name)} ${str(c.type)}`).join(", ")}`);
      for (const r of res.json.relationships ?? []) lines.push(`rel ${str(r.from?.table)}.${JSON.stringify(r.from?.columns)} -> ${str(r.to?.table)}.${JSON.stringify(r.to?.columns)}`);
      if (res.json.next_cursor) lines.push("note: more tables exist");
      if (lines.length === 0) lines.push("no database tables are connected");
      return clip(lines.join("\n"));
    },
    async sql({ sql, max_rows: maxRows = 100 }, signal) {
      const res = await http("POST", "/v1/ontology/query", { sql, max_rows: Math.min(200, Math.max(1, Number(maxRows) || 100)) }, signal);
      if (res.status !== 200) return failure(res);
      const rows = res.json.rows ?? [];
      const tag = registry.add({ kind: "query", sql: str(sql).slice(0, 2000), queryId: str(res.json.provenance?.query_id) || null, source: str(res.json.provenance?.source) });
      const lines = [`[${tag}] ${JSON.stringify(res.json.columns ?? [])}`];
      for (const r of rows.slice(0, MAX_ROWS_SHOWN)) lines.push(JSON.stringify(r));
      if (rows.length > MAX_ROWS_SHOWN || res.json.truncated) lines.push(`[${rows.length} rows${res.json.truncated ? ", truncated by the row cap" : ""}; showing ${Math.min(rows.length, MAX_ROWS_SHOWN)}]`);
      return clip(lines.join("\n"));
    },
  };

  return {
    definitions: TOOL_DEFINITIONS,
    async run(name, args, signal) {
      if (!Object.hasOwn(impl, name)) return `error: unknown tool ${name}`;
      const needs = { search: ["query"], find_text: ["query"], entity: ["urn"], sql: ["sql"] }[name] ?? [];
      for (const k of needs) if (typeof args?.[k] !== "string" || !args[k].trim()) return `error: ${k} is required`;
      try { return await impl[name](args ?? {}, signal); } catch (error) {
        if (signal?.aborted) throw error;
        return "error: the engine did not answer";
      }
    },
  };
}
