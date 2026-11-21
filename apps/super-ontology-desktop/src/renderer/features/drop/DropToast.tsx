import { useEffect, useRef, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, IconButton } from "@open-ontology/design-system";
import { FolderSync, X } from "lucide-react";
import { useErrorText } from "../../hooks/useText";
import type { SuperDriveError } from "../../model/superDriveErrors";
import type { DropOutcome } from "../../model/queue/uploadQueue";

export type SyncState = "idle" | "busy" | "done" | "failed";
export type DropToastState = { id: number; name: string; managed: boolean; outcome: DropOutcome; sync: SyncState; syncError: SuperDriveError | null };

const READ_MS = 7000;
const OFFER_MS = 14_000;

export const skippedTotal = (outcome: DropOutcome): number => Object.values(outcome.skipped).reduce((sum, value) => sum + value, 0);
export const droppedFolders = (outcome: DropOutcome) => outcome.roots.filter((root) => root.kind === "directory");

/**
 * What a drop did, in one small non-blocking toast: how many files were added where, how many items were left out, and, when a folder was
 * dropped, the choice to also keep it in sync as a watched source. The default is the copy that already happened. It leaves by itself unless it
 * is read (hover or focus) or an action is running.
 */
export function DropToast({ toast, onSync, onClose }: { toast: DropToastState; onSync: () => void; onClose: () => void }) {
  const { t } = useLocalization();
  const errorText = useErrorText();
  const { outcome } = toast;
  const folders = droppedFolders(outcome);
  const offer = toast.sync === "idle" && folders.length > 0 && !outcome.error;
  const [held, setHeld] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    window.clearTimeout(timer.current);
    if (held || toast.sync === "busy") return undefined;
    timer.current = window.setTimeout(onClose, offer ? OFFER_MS : READ_MS);
    return () => window.clearTimeout(timer.current);
  }, [held, toast.id, toast.sync, offer, onClose]);

  const skipped = skippedTotal(outcome);
  const name = toast.managed ? t("app.drop.myFiles") : toast.name;
  const headline = outcome.error ? errorText(outcome.error) : outcome.queued === 0 ? t("app.drop.toast.none") : t("app.drop.toast.added", { count: outcome.queued, name });
  return (
    <div className="so-toast" role="status" aria-live="polite" data-testid="drop-toast" data-queued={outcome.queued} data-skipped={skipped} data-sync={toast.sync}
      onMouseEnter={() => setHeld(true)} onMouseLeave={() => setHeld(false)} onFocus={() => setHeld(true)} onBlur={() => setHeld(false)}>
      <div className="so-toast-body">
        <span className="so-toast-title" data-testid="drop-toast-title">{headline}</span>
        {skipped > 0 && !outcome.error && <span className="so-toast-sub" data-testid="drop-toast-skipped">{t("app.drop.skip.title", { count: skipped })}</span>}
        {toast.sync === "busy" && <span className="so-toast-sub">{t("app.drop.toast.syncing")}</span>}
        {toast.sync === "done" && <span className="so-toast-sub" data-testid="drop-toast-synced">{t("app.drop.toast.synced")}</span>}
        {toast.sync === "failed" && <span className="so-toast-sub" data-tone="danger">{t("app.drop.toast.syncFailed")}{toast.syncError ? ` ${errorText(toast.syncError)}` : ""}</span>}
        {(offer || toast.sync === "failed") && folders.length > 0 && (
          <span className="so-toast-actions">
            <Button size="compact" variant="secondary" icon={FolderSync} onClick={onSync} data-testid="drop-keep-in-sync">{t("app.drop.toast.sync", { count: folders.length })}</Button>
          </span>
        )}
      </div>
      <IconButton icon={X} label={t("app.common.close")} size="compact" onClick={onClose} data-testid="drop-toast-close" />
    </div>
  );
}
