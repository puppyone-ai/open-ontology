import { useEffect, useState } from "react";
import type { SuggestionDiff } from "../../../shared/super-ontology/docs-api.generated";
import { useSuperDriveActions } from "../SuperDriveProvider";

type Change = SuggestionDiff["changes"][number];
const BLOCK_PAGE_LIMIT = 500;

/**
 * Text of the blocks a diff points at. The engine sends the new line and a reference, not the old text (`before_ref`), so the old
 * text is read from the file's parsed blocks. A block it cannot read stays absent and the view says so.
 */
export function useChangeContext(diff: SuggestionDiff | null): (change: Change) => string | null | undefined {
  const actions = useSuperDriveActions();
  const [texts, setTexts] = useState<Record<string, string>>({});
  useEffect(() => {
    if (!diff) return undefined;
    let cancelled = false;
    const wanted = new Map<string, Set<string>>();
    for (const change of diff.changes) {
      if (!change.file_id || !change.block_id) continue;
      wanted.set(change.file_id, new Set([...(wanted.get(change.file_id) ?? []), change.block_id]));
    }
    void (async () => {
      const found: Record<string, string> = {};
      for (const [fileId, blockIds] of wanted) {
        let cursor: string | undefined;
        for (let page = 0; page < 5 && blockIds.size > 0; page += 1) {
          const result = await actions.fileBlocks({ fileId, limit: BLOCK_PAGE_LIMIT, ...(cursor ? { cursor } : {}) });
          if (!result.ok) break;
          for (const block of result.value.items) {
            if (blockIds.has(block.id)) { found[`${fileId}:${block.id}`] = block.text; blockIds.delete(block.id); }
          }
          if (!result.value.next_cursor) break;
          cursor = result.value.next_cursor;
        }
      }
      if (!cancelled) setTexts(found);
    })();
    return () => { cancelled = true; };
  }, [actions, diff]);
  return (change) => (change.file_id && change.block_id ? texts[`${change.file_id}:${change.block_id}`] : undefined);
}
