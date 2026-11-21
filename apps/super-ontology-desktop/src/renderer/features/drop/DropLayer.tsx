import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Drive } from "../../../shared/super-ontology/docs-api.generated";
import type { SuperDriveError } from "../../model/superDriveErrors";
import type { DropOutcome } from "../../model/queue/uploadQueue";
import { DropOverlay, type DropOverState } from "./DropOverlay";
import { DropToast, droppedFolders, type DropToastState } from "./DropToast";
import { UploadTray } from "./UploadTray";
import { dragItemCount, isFileDrag, isMyFiles, resolveDropTarget, type DropTarget } from "./dropTarget";

type Failure = { ok: false; error: SuperDriveError };
export type DropLayerDeps = {
  drives: readonly Pick<Drive, "id" | "name" | "backend">[];
  /** The first run has its own drop zone; the layer then only keeps the window from navigating to a dropped file. */
  firstRunOpen: boolean;
  ensureMyFiles: () => Promise<{ ok: true; value: { id: string } } | Failure>;
  enqueue: (driveId: string, folder: string, files: File[]) => Promise<DropOutcome>;
  /** Registers a dropped folder as a watched local source and takes back the copies this drop made of it (`batchIds`). */
  keepInSync: (root: { path: string; name: string }, batchIds: string[]) => Promise<{ ok: true } | Failure>;
};

/** Hides when no dragover has been seen for this long: a drag that leaves the window quickly can lose its last dragleave. */
const WATCHDOG_MS = 1500;
const emptyOutcome = (error: SuperDriveError | null): DropOutcome => ({ queued: 0, skipped: { hidden: 0, links: 0, junk: 0, temporary: 0, special: 0, unreadable: 0 }, roots: [], emptyFolders: 0, batchIds: [], error });

/**
 * Window-wide drop target. Dragging files or folders anywhere over the app shows one overlay naming where the drop goes: My files by default, a
 * source when the pointer is over that source (`data-drop-*` attributes, see dropTarget.ts), or why a source cannot take files. A drop copies
 * everything (folders keep their structure; the main process walks them) and answers with a toast. Everything lives in one window listener, so a
 * screen needs only the attributes, never its own handlers, and a file dropped on the wrong spot never navigates the window away.
 */
export function DropLayer({ drives, firstRunOpen, ensureMyFiles, enqueue, keepInSync }: DropLayerDeps) {
  const [over, setOver] = useState<DropOverState | null>(null);
  const [toast, setToast] = useState<DropToastState | null>(null);
  const serial = useRef(0);
  const myFiles = useMemo(() => drives.find((drive) => isMyFiles(drive)) ?? null, [drives]);
  const live = useRef({ myFiles, firstRunOpen, ensureMyFiles, enqueue });
  live.current = { myFiles, firstRunOpen, ensureMyFiles, enqueue };

  const handle = useCallback(async (target: Extract<DropTarget, { kind: "drive" }>, files: File[]) => {
    serial.current += 1;
    const id = serial.current;
    let driveId = target.driveId;
    if (!driveId) {
      const ensured = await live.current.ensureMyFiles();
      if (!ensured.ok) { setToast({ id, name: target.name, managed: true, outcome: emptyOutcome(ensured.error), sync: "idle", syncError: null }); return; }
      driveId = ensured.value.id;
    }
    const outcome = await live.current.enqueue(driveId, target.folder, files);
    setToast({ id, name: target.name, managed: target.managed, outcome, sync: "idle", syncError: null });
  }, []);

  useEffect(() => {
    let depth = 0;
    let watchdog: number | undefined;
    let marked: Element | null = null;
    const mark = (element: Element | null) => {
      if (element === marked) return;
      marked?.removeAttribute("data-drop-over");
      element?.setAttribute("data-drop-over", "");
      marked = element;
    };
    const clear = () => {
      depth = 0;
      window.clearTimeout(watchdog);
      mark(null);
      setOver(null);
    };
    const evaluate = (event: DragEvent) => {
      const { target, element } = resolveDropTarget(event.target instanceof Element ? event.target : null, live.current.myFiles);
      mark(element);
      const count = dragItemCount(event.dataTransfer);
      setOver((previous) => (previous && previous.count === count && sameTarget(previous.target, target) ? previous : { target, count }));
      window.clearTimeout(watchdog);
      watchdog = window.setTimeout(clear, WATCHDOG_MS);
      return target;
    };
    const onEnter = (event: DragEvent) => {
      if (!isFileDrag(event.dataTransfer)) return;
      event.preventDefault();
      if (live.current.firstRunOpen) return;
      depth += 1;
      evaluate(event);
    };
    const onOver = (event: DragEvent) => {
      if (!isFileDrag(event.dataTransfer)) return;
      event.preventDefault();
      if (live.current.firstRunOpen) return;
      const target = evaluate(event);
      if (event.dataTransfer) event.dataTransfer.dropEffect = target.kind === "rejected" ? "none" : "copy";
    };
    const onLeave = (event: DragEvent) => {
      if (!isFileDrag(event.dataTransfer) || live.current.firstRunOpen) return;
      depth = Math.max(0, depth - 1);
      if (depth === 0) clear();
    };
    const onDrop = (event: DragEvent) => {
      if (!isFileDrag(event.dataTransfer)) return;
      // Always: a file dropped where nothing listens would otherwise make the window navigate to it.
      event.preventDefault();
      if (live.current.firstRunOpen) { clear(); return; }
      // The File objects are copied now: the list is empty again once this handler returns.
      const files = Array.from(event.dataTransfer?.files ?? []);
      const { target } = resolveDropTarget(event.target instanceof Element ? event.target : null, live.current.myFiles);
      clear();
      if (target.kind === "drive" && files.length > 0) void handle(target, files);
    };
    window.addEventListener("dragenter", onEnter);
    window.addEventListener("dragover", onOver);
    window.addEventListener("dragleave", onLeave);
    window.addEventListener("drop", onDrop);
    window.addEventListener("dragend", clear);
    return () => {
      window.removeEventListener("dragenter", onEnter);
      window.removeEventListener("dragover", onOver);
      window.removeEventListener("dragleave", onLeave);
      window.removeEventListener("drop", onDrop);
      window.removeEventListener("dragend", clear);
      clear();
    };
  }, [handle]);

  const sync = useCallback(async () => {
    const current = toast;
    if (!current) return;
    setToast({ ...current, sync: "busy", syncError: null });
    // One at a time: each folder needs its own one-use grant.
    let failure: SuperDriveError | null = null;
    for (const root of droppedFolders(current.outcome)) {
      const result = await keepInSync({ path: root.path, name: root.name }, current.outcome.batchIds);
      if (!result.ok) { failure = result.error; break; }
    }
    setToast((now) => (now && now.id === current.id ? { ...now, sync: failure ? "failed" : "done", syncError: failure } : now));
  }, [toast, keepInSync]);

  return (
    <>
      <DropOverlay over={over} />
      <div className="so-float">
        {toast && <DropToast toast={toast} onSync={() => { void sync(); }} onClose={() => setToast(null)} />}
        <UploadTray />
      </div>
    </>
  );
}

function sameTarget(a: DropTarget, b: DropTarget): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "rejected" && b.kind === "rejected") return a.name === b.name && a.reason === b.reason;
  if (a.kind === "drive" && b.kind === "drive") return a.driveId === b.driveId && a.folder === b.folder && a.managed === b.managed;
  return false;
}
