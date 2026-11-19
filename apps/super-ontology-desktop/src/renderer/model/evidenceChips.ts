import type { Evidence } from "../../shared/super-ontology/docs-api.generated";

export type EvidenceGroup = { fileId: string; path: string; name: string; anchors: string[] };

/** "L6–L6" and "L6-L6" read as "L6"; real ranges keep their end ("L6–L9"). Other anchors (pages, cells) pass through. */
export function collapseAnchor(raw: string): string {
  const match = /^L(\d+)\s*[-–—]\s*L?(\d+)$/u.exec(raw.trim());
  if (!match) return raw.trim();
  return match[1] === match[2] ? `L${match[1]}` : `L${match[1]}–L${match[2]}`;
}

function anchorOf(evidence: Evidence): string {
  const at = evidence.src.indexOf("#");
  return at === -1 ? "" : collapseAnchor(evidence.src.slice(at + 1).replace(/\]$/u, ""));
}

/** Several pieces of evidence from one file become one chip carrying all its anchors, in first-seen order and without repeats. */
export function groupEvidence(evidence: Evidence[]): EvidenceGroup[] {
  const groups = new Map<string, EvidenceGroup>();
  for (const item of evidence) {
    const key = item.file_id || item.path;
    let group = groups.get(key);
    if (!group) {
      group = { fileId: item.file_id, path: item.path, name: item.path.slice(item.path.lastIndexOf("/") + 1), anchors: [] };
      groups.set(key, group);
    }
    const anchor = anchorOf(item);
    if (anchor && !group.anchors.includes(anchor)) group.anchors.push(anchor);
  }
  return [...groups.values()];
}
