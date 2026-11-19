import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";

/** Above this many rows the list mounts only a window of them (§4.6, `SIDEBAR_VIRTUALIZATION_THRESHOLD`). */
export const VIRTUALIZATION_THRESHOLD = 200;
/** Never more than this many rows are mounted at a time (`SIDEBAR_VIRTUALIZATION_MAX_MOUNTED_ROWS`). */
export const VIRTUALIZATION_MAX_MOUNTED_ROWS = 120;
const OVERSCAN_ROWS = 10;
const FALLBACK_ROW_HEIGHT = 32;
const FALLBACK_VIEWPORT_HEIGHT = 640;

export type VirtualWindow = {
  /** First and one-past-last mounted row index in `[0, count)`. */
  start: number;
  end: number;
  rowHeight: number;
  /** Scroll so that the row is inside the mounted window; the caller focuses it afterwards. */
  ensureVisible: (index: number) => void;
};

/**
 * Window over `count` equally tall rows. `rowsRef` is the element holding the rows; its nearest scrolling ancestor
 * (`scrollParentSelector`) drives the window. Row positions are measured from the DOM rather than assumed.
 */
export function useVirtualWindow({ count, enabled, rowsRef, scrollParentSelector, rowSelector }: {
  count: number; enabled: boolean; rowsRef: RefObject<HTMLElement | null>; scrollParentSelector: string; rowSelector: string;
}): VirtualWindow {
  const [range, setRange] = useState({ start: 0, end: Math.min(count, VIRTUALIZATION_MAX_MOUNTED_ROWS), rowHeight: FALLBACK_ROW_HEIGHT });
  const rowHeightRef = useRef(FALLBACK_ROW_HEIGHT);

  const parent = useCallback((): HTMLElement | null => {
    const element = rowsRef.current;
    return element ? (element.closest(scrollParentSelector) as HTMLElement | null) ?? element : null;
  }, [rowsRef, scrollParentSelector]);

  const measure = useCallback(() => {
    const element = rowsRef.current;
    const scroller = parent();
    if (!element || !scroller || !enabled) return;
    const sample = element.querySelector<HTMLElement>(rowSelector);
    if (sample && sample.offsetHeight > 0) rowHeightRef.current = sample.offsetHeight;
    const rowHeight = rowHeightRef.current;
    const scrolled = Math.max(0, scroller.getBoundingClientRect().top - element.getBoundingClientRect().top);
    const viewport = scroller.clientHeight || FALLBACK_VIEWPORT_HEIGHT;
    const visible = Math.ceil(viewport / rowHeight);
    const size = Math.min(VIRTUALIZATION_MAX_MOUNTED_ROWS, visible + 2 * OVERSCAN_ROWS);
    const first = Math.max(0, Math.min(Math.max(0, count - size), Math.floor(scrolled / rowHeight) - OVERSCAN_ROWS));
    setRange((previous) => (previous.start === first && previous.end === Math.min(count, first + size) && previous.rowHeight === rowHeight
      ? previous : { start: first, end: Math.min(count, first + size), rowHeight }));
  }, [count, enabled, parent, rowSelector, rowsRef]);

  useLayoutEffect(() => { measure(); }, [measure]);
  useEffect(() => {
    if (!enabled) return undefined;
    const scroller = parent();
    if (!scroller) return undefined;
    scroller.addEventListener("scroll", measure, { passive: true });
    window.addEventListener("resize", measure);
    return () => { scroller.removeEventListener("scroll", measure); window.removeEventListener("resize", measure); };
  }, [enabled, measure, parent]);

  const ensureVisible = useCallback((index: number) => {
    if (!enabled) return;
    const element = rowsRef.current;
    const scroller = parent();
    if (!element || !scroller) return;
    const rowHeight = rowHeightRef.current;
    const top = element.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop;
    const rowTop = top + index * rowHeight;
    const viewTop = scroller.scrollTop;
    const viewBottom = viewTop + (scroller.clientHeight || FALLBACK_VIEWPORT_HEIGHT);
    if (rowTop < viewTop) scroller.scrollTop = rowTop;
    else if (rowTop + rowHeight > viewBottom) scroller.scrollTop = rowTop + rowHeight - (scroller.clientHeight || FALLBACK_VIEWPORT_HEIGHT);
    // Mount the row now: the scroll event arrives later than the focus call.
    const size = VIRTUALIZATION_MAX_MOUNTED_ROWS;
    setRange((previous) => {
      if (index >= previous.start && index < previous.end) return previous;
      const first = Math.max(0, Math.min(Math.max(0, count - size), index - Math.floor(size / 2)));
      return { start: first, end: Math.min(count, first + size), rowHeight: previous.rowHeight };
    });
  }, [count, enabled, parent, rowsRef]);

  if (!enabled) return { start: 0, end: count, rowHeight: FALLBACK_ROW_HEIGHT, ensureVisible };
  const end = Math.min(count, range.end || count);
  return { start: Math.min(range.start, Math.max(0, end - 1)), end, rowHeight: range.rowHeight, ensureVisible };
}
