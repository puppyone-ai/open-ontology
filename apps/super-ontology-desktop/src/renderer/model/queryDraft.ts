/** A SQL text handed from another screen (an Ask citation) to Ontology › Query: read once by the editor when it opens. */
let draft: string | null = null;
export const queryDraft = {
  set(sql: string) { draft = sql; },
  take(): string | null { const value = draft; draft = null; return value; },
};

/** The SELECT that shows the row an answer cited. Identifiers are quoted; the key is a string literal. */
export function rowQuery(table: string, keyColumn: string, pk: string): string {
  const quote = (name: string) => `"${name.replace(/"/gu, '""')}"`;
  const qualified = table.split(".").map(quote).join(".");
  return `SELECT * FROM ${qualified} WHERE ${quote(keyColumn || "id")} = '${pk.replace(/'/gu, "''")}' LIMIT 10`;
}
