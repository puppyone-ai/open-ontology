import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import type { TreeDir } from "../../../shared/super-ontology/docs-api.generated";
import type { FileRecord } from "../superDriveState";
import { presentFile } from "../superDriveFileView";
import type { MenuActionId, MenuTarget } from "./fileMenuModel";

export type CollectionItem = { kind: "dir"; dir: TreeDir } | { kind: "file"; file: FileRecord };

export type FileCollectionOptions = {
  dirs: readonly TreeDir[];
  files: readonly FileRecord[];
  selected: ReadonlySet<string>;
  hasMore: boolean;
  /** A full disk or a Drive being removed: the menu offers no retry or sync (§4.6). */
  writesBlocked?: boolean;
  /** Tiles per row: 1 for the list. With more than one, the left and right arrows move by one tile. */
  columns: () => number;
  /** The virtual list calls this before it focuses a row that may not be mounted. */
  ensureVisible?: (index: number) => void;
  onOpenDir: (path: string) => void;
  onOpenFile: (fileId: string) => void;
  onSelect: (ids: string[], anchorId: string | null) => void;
  onLoadMore: () => void;
  onContextAction?: (action: MenuActionId, target: MenuTarget) => void;
  onUp?: () => void;
  onToggleDetails?: () => void;
  onEscape?: () => void;
  onRetryShortcut?: () => void;
  onFocusSearch?: () => void;
};

const folderName = (path: string) => path.slice(path.lastIndexOf("/") + 1);

export const viewOfFile = (file: FileRecord) => presentFile({
  state: file.state, state_code: file.state_code, retryable: file.retryable, blocked_reason: file.blocked_reason, pending_review: file.pending_review, progress: file.progress,
});

/**
 * One selection, focus, keyboard and context menu model for the list and the grid (§3.2.2: "shared model"). Rows
 * are addressed by their index in `[...dirs, ...files]`; the view only decides how they look and which ones are mounted.
 */
export function useFileCollection(options: FileCollectionOptions) {
  const { dirs, files, selected, hasMore, writesBlocked = false, columns, ensureVisible, onOpenDir, onOpenFile, onSelect, onLoadMore, onContextAction, onUp, onToggleDetails, onEscape, onRetryShortcut, onFocusSearch } = options;
  // Rebuilt only when the listing changes: with 10,000 files this array is too big to make on every scroll frame (F-S13-01).
  const items = useMemo<CollectionItem[]>(() => [...dirs.map((dir) => ({ kind: "dir" as const, dir })), ...files.map((file) => ({ kind: "file" as const, file }))], [dirs, files]);
  const [menu, setMenu] = useState<{ target: MenuTarget; x: number; y: number; index: number } | null>(null);
  const [focusIndex, setFocusIndex] = useState(0);
  const itemRefs = useRef<Array<HTMLDivElement | null>>([]);
  const pendingFocus = useRef(false);

  useEffect(() => {
    if (!pendingFocus.current) return;
    const element = itemRefs.current[focusIndex];
    if (!element) return;
    pendingFocus.current = false;
    element.focus();
  });

  const focusItem = (index: number) => {
    const next = Math.max(0, Math.min(items.length - 1, index));
    setFocusIndex(next);
    ensureVisible?.(next);
    const element = itemRefs.current[next];
    if (element) element.focus();
    else pendingFocus.current = true;
    return next;
  };
  const fileIdAt = (index: number) => {
    const item = items[index];
    return item?.kind === "file" ? item.file.id : null;
  };
  const selectRange = (from: number, to: number) => {
    const [low, high] = from <= to ? [from, to] : [to, from];
    onSelect(items.slice(low, high + 1).flatMap((item) => (item.kind === "file" ? [item.file.id] : [])), fileIdAt(from));
  };
  const anchorIndex = () => {
    const anchor = [...selected][0];
    const index = items.findIndex((item) => item.kind === "file" && item.file.id === anchor);
    return index === -1 ? focusIndex : index;
  };
  const activate = (index: number) => {
    const item = items[index];
    if (!item) return;
    if (item.kind === "dir") onOpenDir(item.dir.path);
    else onOpenFile(item.file.id);
  };
  const targetAt = (index: number): MenuTarget | null => {
    const entry = items[index];
    if (!entry) return null;
    if (entry.kind === "dir") return { kind: "dir", path: entry.dir.path, writesBlocked };
    if (selected.size > 1 && selected.has(entry.file.id)) {
      const chosen = files.filter((file) => selected.has(file.id)).map(viewOfFile);
      return { kind: "multi", fileIds: [...selected], retryable: writesBlocked ? 0 : chosen.filter((view) => view.canRetry).length, safeRetryable: writesBlocked ? 0 : chosen.filter((view) => view.canRetrySafe).length };
    }
    const view = viewOfFile(entry.file);
    return { kind: "file", fileId: entry.file.id, path: entry.file.path, name: entry.file.name ?? folderName(entry.file.path), view: writesBlocked ? { ...view, canRetry: false, canRetrySafe: false } : view, duplicateOf: entry.file.duplicate_of ?? null };
  };
  const openMenuAt = (index: number, x: number, y: number) => {
    const target = targetAt(index);
    if (target && onContextAction) setMenu({ target, x, y, index });
  };
  const closeMenu = useCallback(() => setMenu(null), []);
  const closeMenuAndFocus = () => { const index = menu?.index ?? focusIndex; setMenu(null); itemRefs.current[index]?.focus(); };

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const modifier = event.metaKey || event.ctrlKey;
    if (event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey)) {
      event.preventDefault();
      const rect = itemRefs.current[focusIndex]?.getBoundingClientRect();
      openMenuAt(focusIndex, (rect?.left ?? 0) + 24, (rect?.bottom ?? 0));
      return;
    }
    if (modifier && event.key.toLowerCase() === "i") { event.preventDefault(); onToggleDetails?.(); return; }
    if (modifier && event.key.toLowerCase() === "f") { event.preventDefault(); onFocusSearch?.(); return; }
    if (event.key === "Backspace" || (modifier && event.key === "ArrowUp")) { event.preventDefault(); onUp?.(); return; }
    if (event.key === "Escape") { event.preventDefault(); onEscape?.(); return; }
    if (!modifier && !event.altKey && event.key.toLowerCase() === "r" && selected.size > 0) { event.preventDefault(); onRetryShortcut?.(); return; }
    const perRow = Math.max(1, columns());
    const move = (delta: number) => {
      event.preventDefault();
      const next = focusItem(focusIndex + delta);
      if (event.shiftKey) selectRange(anchorIndex(), next);
    };
    switch (event.key) {
      case "ArrowDown": move(perRow); return;
      case "ArrowUp": move(-perRow); return;
      case "ArrowRight": if (perRow > 1) move(1); return;
      case "ArrowLeft": if (perRow > 1) move(-1); return;
      case "Home": event.preventDefault(); focusItem(0); return;
      case "End": event.preventDefault(); focusItem(items.length - 1); if (hasMore) onLoadMore(); return;
      case "PageDown": event.preventDefault(); focusItem(focusIndex + 10 * perRow); return;
      case "PageUp": event.preventDefault(); focusItem(focusIndex - 10 * perRow); return;
      case "Enter": event.preventDefault(); activate(focusIndex); return;
      case " ": {
        event.preventDefault();
        const id = fileIdAt(focusIndex);
        if (!id) return;
        const next = new Set(selected);
        if (next.has(id)) next.delete(id); else next.add(id);
        onSelect([...next], id);
        return;
      }
      case "a": case "A":
        if (modifier) { event.preventDefault(); onSelect(files.map((file) => file.id), files[0]?.id ?? null); }
        return;
      default:
    }
  };

  const onItemClick = (event: MouseEvent, index: number) => {
    setFocusIndex(index);
    const item = items[index];
    if (item?.kind !== "file") return;
    if (event.shiftKey) { selectRange(anchorIndex(), index); return; }
    if (event.metaKey || event.ctrlKey) {
      const next = new Set(selected);
      if (next.has(item.file.id)) next.delete(item.file.id); else next.add(item.file.id);
      onSelect([...next], item.file.id);
      return;
    }
    onSelect([item.file.id], item.file.id);
    onOpenFile(item.file.id);
  };
  const onItemContextMenu = (event: MouseEvent, index: number) => {
    if (!onContextAction) return;
    event.preventDefault();
    setFocusIndex(index);
    openMenuAt(index, event.clientX, event.clientY);
  };

  return { items, itemRefs, focusIndex, setFocusIndex, menu, closeMenu, closeMenuAndFocus, openMenuAt, onKeyDown, onItemClick, onItemContextMenu, activate };
}
