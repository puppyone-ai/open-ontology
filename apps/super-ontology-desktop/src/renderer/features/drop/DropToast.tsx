import { useEffect, useRef, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, IconButton, ProgressBar } from "@open-ontology/design-system";
import { FolderSync, X } from "lucide-react";
import { useErrorText } from "../../hooks/useText";
import { formatBytes } from "../../model/format";
import type { SuperDriveError } from "../../model/superDriveErrors";
import type { DropSummary } from "../../model/queue/uploadQueue";

export type SyncState = "idle" | "busy" | "done" | "failed";
/** What the person chose for the big folders of the drop: link them (no copy), or copy them. */
export type ChoiceState = "none" | "linking" | "linked" | "copying";
export type DropToastState = {
  id: number;
  name: string;
  managed: boolean;
  /** False while the drop is being handed to the main process ("Preparing…" with nothing found yet); true once its batches exist. */
  started: boolean;
  /** The batches the drop started. */
  batchIds: string[];
  /** The drop could not be started at all (no engine, ...). */
  error: SuperDriveError | null;
  sync: SyncState;
  syncError: SuperDriveError | null;
  choice: ChoiceState;
  /** Folders and files that were linked as sources. */
  linked: { name: string; files: number }[];
};

const READ_MS = 7000;
const OFFER_MS = 14_000;

/** The folders of the drop that were copied and can still be kept in sync instead (small ones: the copy already happened). */
export const droppedFolders = (summary: DropSummary) => summary.folders;

/**
 * What a drop does, in one small non-blocking toast, from the frame of the drop on: "Preparing…" with the files found so far while the main
 * process walks it, then, for a big folder, the choice (link it, which is the default, or copy it), then how many files were added where and how
 * many items were left out, and, for a small folder, the offer to also keep it in sync. It leaves by itself unless it is read (hover or focus),
 * waits for a choice, or an action is running.
 */
export function DropToast({ toast, summary, onSync, onLink, onCopy, onCancel, onClose }: {
  toast: DropToastState; summary: DropSummary; onSync: () => void; onLink: () => void; onCopy: () => void; onCancel: () => void; onClose: () => void;
}) {
  const { t, formatNumber } = useLocalization();
  const errorText = useErrorText();
  const sizeOf = (bytes: number) => formatBytes(bytes, { t, formatNumber });
  const failure = toast.error ?? null;
  const folders = droppedFolders(summary);
  const choosing = !failure && summary.phase === "choice" && toast.choice === "none";
  const offer = toast.sync === "idle" && toast.choice === "none" && folders.length > 0 && !failure && summary.phase === "done" && summary.pending.length === 0;
  const busy = toast.sync === "busy" || toast.choice === "linking";
  const working = !failure && (summary.phase === "preparing" || summary.phase === "copying");
  const [held, setHeld] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    window.clearTimeout(timer.current);
    if (held || busy || choosing || working) return undefined;
    timer.current = window.setTimeout(onClose, offer ? OFFER_MS : READ_MS);
    return () => window.clearTimeout(timer.current);
  }, [held, toast.id, toast.sync, toast.choice, busy, choosing, working, offer, summary.phase, onClose]);

  const skipped = summary.skipped;
  const name = toast.managed ? t("app.drop.myFiles") : toast.name;
  const problem = failure ?? (summary.problem ? ({ code: summary.problem.code, retryable: false, details: {} } satisfies SuperDriveError) : null);
  let headline: string;
  if (failure) headline = errorText(failure);
  else if (toast.choice === "linked") headline = t("app.drop.toast.linked", { count: toast.linked.length, name: toast.linked[0]?.name ?? "", files: toast.linked.reduce((sum, entry) => sum + entry.files, 0) });
  else if (summary.phase === "preparing") headline = t("app.drop.toast.preparing");
  else if (choosing || toast.choice === "linking") headline = t("app.drop.toast.large", { count: summary.pending.length || toast.linked.length || 1, name: summary.pending[0]?.name ?? "", files: summary.found, size: sizeOf(summary.bytes) });
  else if (summary.phase === "copying") headline = t("app.drop.toast.copying", { done: summary.settled, total: summary.queued });
  else if (summary.queued === 0 && summary.found === 0) headline = t("app.drop.toast.none");
  else headline = t("app.drop.toast.added", { count: summary.done, name });
  const ratio = summary.phase === "copying" && summary.queued > 0 ? summary.settled / summary.queued : null;
  return (
    <div className="so-toast" role="status" aria-live="polite" data-testid="drop-toast" data-phase={failure ? "failed" : summary.phase} data-choice={toast.choice} data-queued={summary.queued} data-found={summary.found} data-skipped={skipped} data-sync={toast.sync}
      onMouseEnter={() => setHeld(true)} onMouseLeave={() => setHeld(false)} onFocus={() => setHeld(true)} onBlur={() => setHeld(false)}>
      <div className="so-toast-body">
        <span className="so-toast-title" data-testid="drop-toast-title">{headline}</span>
        {!failure && (summary.phase === "preparing" || choosing) && summary.found > 0 && <span className="so-toast-sub" data-testid="drop-toast-found" data-count={summary.found}>{t("app.drop.toast.found", { count: summary.found })}</span>}
        {working && !failure && <ProgressBar label={headline} value={ratio} />}
        {skipped > 0 && !failure && summary.phase !== "preparing" && <span className="so-toast-sub" data-testid="drop-toast-skipped" data-count={skipped}>{t("app.drop.skip.title", { count: skipped })}</span>}
        {problem && !failure && <span className="so-toast-sub" data-tone="danger" role="alert" data-testid="drop-toast-problem">{errorText(problem)}{summary.problem?.needed !== undefined && summary.problem.free !== undefined ? ` ${t("app.drop.toast.space", { needed: sizeOf(summary.problem.needed), free: sizeOf(summary.problem.free) })}` : ""}</span>}
        {toast.choice === "linking" && <span className="so-toast-sub">{t("app.drop.toast.linking")}</span>}
        {toast.choice === "linked" && <span className="so-toast-sub" data-testid="drop-toast-linked">{t("app.drop.toast.linkedSub")}</span>}
        {toast.sync === "busy" && <span className="so-toast-sub">{t("app.drop.toast.syncing")}</span>}
        {toast.sync === "done" && toast.choice === "none" && <span className="so-toast-sub" data-testid="drop-toast-synced">{t("app.drop.toast.synced")}</span>}
        {toast.sync === "failed" && <span className="so-toast-sub" data-tone="danger">{t("app.drop.toast.syncFailed")}{toast.syncError ? ` ${errorText(toast.syncError)}` : ""}</span>}
        {(choosing || (toast.sync === "failed" && toast.choice !== "copying")) && summary.pending.length > 0 && (
          <span className="so-toast-actions" data-testid="drop-choice">
            <Button size="compact" variant="primary" icon={FolderSync} onClick={onLink} data-testid="drop-link-folder">{t("app.drop.toast.link", { count: summary.pending.length })}</Button>
            <Button size="compact" variant="secondary" onClick={onCopy} data-testid="drop-copy-folder">{t("app.drop.toast.copyLarge", { name })}</Button>
          </span>
        )}
        {(offer || (toast.sync === "failed" && summary.pending.length === 0)) && folders.length > 0 && (
          <span className="so-toast-actions">
            <Button size="compact" variant="secondary" icon={FolderSync} onClick={onSync} data-testid="drop-keep-in-sync">{t("app.drop.toast.sync", { count: folders.length })}</Button>
          </span>
        )}
      </div>
      {working || choosing ? <IconButton icon={X} label={t("app.common.cancel")} size="compact" onClick={onCancel} data-testid="drop-toast-cancel" /> : <IconButton icon={X} label={t("app.common.close")} size="compact" onClick={onClose} data-testid="drop-toast-close" />}
    </div>
  );
}
