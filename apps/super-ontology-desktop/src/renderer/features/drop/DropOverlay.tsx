import { useLocalization } from "@open-ontology/design-system/localization";
import { ArrowDownToLine, Ban } from "lucide-react";
import type { DropTarget } from "./dropTarget";

export type DropOverState = { target: DropTarget; count: number };

/** The one calm full-window cover while files or folders are dragged over the app (25 §4): a hairline border, a light accent tint, where the drop goes. */
export function DropOverlay({ over }: { over: DropOverState | null }) {
  const { t } = useLocalization();
  if (!over) return null;
  const { target, count } = over;
  const rejected = target.kind === "rejected";
  const title = rejected ? t("app.drop.overlay.rejected", { name: target.name })
    : target.managed ? t("app.drop.overlay.myFiles") : t("app.drop.overlay.source", { name: target.name });
  const Icon = rejected ? Ban : ArrowDownToLine;
  return (
    <div className="so-drop-overlay" data-testid="drop-overlay" data-state={rejected ? "rejected" : "ready"} data-target={target.kind === "drive" ? (target.managed ? "my-files" : "source") : target.reason} role="status" aria-live="polite">
      <div className="so-drop-card">
        <span className="so-drop-icon"><Icon size={20} strokeWidth={1.5} aria-hidden="true" /></span>
        <span className="so-drop-title so-truncate" data-testid="drop-overlay-title">{title}</span>
        {rejected ? <span className="so-drop-detail">{t(`app.drop.reject.${target.reason}`)}</span> : (
          <>
            <span className="so-drop-detail" data-testid="drop-overlay-count" data-count={count}>{t("app.drop.overlay.count", { count })}</span>
            <span className="so-drop-hint">{t("app.drop.overlay.hint")}</span>
          </>
        )}
      </div>
    </div>
  );
}
