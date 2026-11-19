import type { EngineAnchor, EngineBlock } from "../../../shared/super-ontology/bridge";

type T = (id: string, values?: Record<string, string | number>) => string;

/** Where a block sits in its source, in words (§3.4.2): lines, page, paragraphs, slide, or cells. */
export function anchorLabel(anchor: EngineAnchor | undefined, t: T): string {
  if (!anchor) return "";
  switch (anchor.kind) {
    case "text":
      return anchor.line_start === anchor.line_end
        ? t("super-drive.details.anchor.line", { start: anchor.line_start })
        : t("super-drive.details.anchor.lines", { start: anchor.line_start, end: anchor.line_end });
    case "pdf":
      return t("super-drive.details.anchor.page", { page: anchor.page });
    case "docx":
      if (anchor.table_index !== undefined) return t("super-drive.details.anchor.cell", { table: anchor.table_index + 1, cells: anchor.cell_range ?? "" });
      return anchor.para_start === anchor.para_end
        ? t("super-drive.details.anchor.paragraph", { start: anchor.para_start + 1 })
        : t("super-drive.details.anchor.paragraphs", { start: anchor.para_start + 1, end: anchor.para_end + 1 });
    case "slide":
      return t(anchor.notes ? "super-drive.details.anchor.notes" : "super-drive.details.anchor.slide", { slide: anchor.slide });
    case "cells":
      return anchor.sheet ? t("super-drive.details.anchor.cells", { sheet: anchor.sheet, range: anchor.range }) : t("super-drive.details.anchor.range", { range: anchor.range });
    default:
      return "";
  }
}

/** The region of a block on its page, frame or image, in the engine's units (OCR anchors, F-S9-01); `null` when the anchor has none. */
export function anchorRegion(anchor: EngineAnchor | undefined): string | null {
  const box = (anchor as { bbox?: unknown } | undefined)?.bbox;
  if (!Array.isArray(box) || box.length !== 4 || !box.every((value) => Number.isFinite(value))) return null;
  return `${box[0]}, ${box[1]} – ${box[2]}, ${box[3]}`;
}

/** The page a block belongs to when its format has pages (PDF page, slide number); `null` otherwise. */
export function pageOfBlock(block: Pick<EngineBlock, "anchor">): number | null {
  const anchor = block.anchor;
  if (anchor?.kind === "pdf") return anchor.page;
  if (anchor?.kind === "slide") return anchor.slide;
  return null;
}

export const BLOCK_KINDS = [
  "heading", "paragraph", "list_item", "quote", "code", "table", "caption", "frontmatter", "slide_title", "slide_notes", "sheet_table", "kv",
  "page_header", "page_footer", "footnote",
] as const;

/** First rows of a table block for the preview (`table.rows` of the doc-ir grid, when the engine sent it). */
export function tablePreviewRows(block: EngineBlock, limit = 5): string[][] {
  const rows = (block.table as { rows?: unknown[][] } | undefined)?.rows;
  if (!Array.isArray(rows)) return [];
  return rows.slice(0, limit).map((row) => (Array.isArray(row) ? row.map((cell) => (typeof cell === "string" ? cell : cell === null || cell === undefined ? "" : typeof cell === "object" && "text" in (cell as object) ? String((cell as { text: unknown }).text) : String(cell))) : []));
}
