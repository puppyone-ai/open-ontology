import type { ReactNode } from "react";

/**
 * The BRIEF section "Other related" (the engine writes it as 其他相关, in the BRIEF's own language). With the vector model on, the
 * engine fills it from the entities a graph walk did not reach as well, ranked by local semantic similarity. The BRIEF does not say
 * which line came from which, so the preview marks the section, not the lines (F-S13-02). The mark is only drawn: the BRIEF text,
 * its source labels and what is copied from it are exactly what the engine produced.
 */
const RELATED_HEADINGS = new Set(["其他相关", "other related", "related"]);

/** The plain text of a heading's children (strings and nested elements). */
export function textOf(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textOf).join("");
  if (node !== null && typeof node === "object" && "props" in node) return textOf((node as { props: { children?: ReactNode } }).props.children);
  return "";
}

export const isRelatedHeading = (node: ReactNode): boolean => RELATED_HEADINGS.has(textOf(node).trim().toLowerCase());
