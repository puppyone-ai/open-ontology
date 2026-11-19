import { useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, DefinitionList, IconButton, Notice, Skeleton, StatusDot } from "@open-ontology/design-system";
import { RotateCw, X } from "lucide-react";
import { useErrorText } from "../../hooks/useText";
import { useFileDetail } from "../../model/details/useFileDetail";
import { formatBytes } from "../../model/format";
import { useSuperDriveActions, useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { fileView, TONE, useFileStateText } from "./fileView";

/** What the engine made of one file: state and why, size, and the entities it contributed to. Retry is offered when it can help. */
export function FilePanel({ fileId, onClose }: { fileId: string; onClose: () => void }) {
  const t$ = useLocalization();
  const { t, formatDate } = t$;
  const actions = useSuperDriveActions();
  const errorText = useErrorText();
  const stateText = useFileStateText();
  const record = useSuperDriveSelector((s) => s.files[fileId] ?? null);
  const { detail, error, loading } = useFileDetail(fileId);
  const [retrying, setRetrying] = useState(false);
  const view = record ? fileView(record) : null;
  const retry = async () => { setRetrying(true); await actions.retryFiles([fileId]); setRetrying(false); };

  return (
    <Card className="so-metric" aria-label={t("app.sources.panel.title")} role="complementary" data-testid="file-panel" style={{ position: "sticky", top: 0 }}>
      <div className="so-row">
        <h2 className="so-section-title so-grow so-truncate">{record?.name ?? record?.path ?? <Skeleton width={160} height={16} />}</h2>
        <IconButton icon={X} label={t("app.common.close")} size="compact" onClick={onClose} />
      </div>
      {view && <StatusDot tone={TONE[view.tone]} label={stateText(view)} pulse={view.busy} />}
      {view?.canRetry && <div><Button icon={RotateCw} loading={retrying} onClick={() => { void retry(); }}>{t("app.sources.panel.retry")}</Button></div>}
      {error && <Notice tone="danger" title={t("app.common.loadFailed")}>{errorText(error)}</Notice>}
      {record && (
        <DefinitionList items={[
          { label: t("app.sources.panel.path"), value: <span className="so-mono">{record.path}</span> },
          { label: t("app.sources.col.size"), value: formatBytes(record.size, t$) || "–" },
          { label: t("app.sources.col.modified"), value: record.modified_at ? formatDate(new Date(record.modified_at), { dateStyle: "medium", timeStyle: "short" }) : "–" },
          ...(record.format ? [{ label: t("app.sources.panel.format"), value: record.format }] : []),
        ]} />
      )}
      {loading && !detail && <Skeleton height={12} />}
      {detail && (() => {
        const entities = (detail as unknown as { entities?: Array<{ id: string; name: string }> }).entities ?? [];
        return entities.length > 0 ? (
          <div className="so-stack">
            <h3 className="so-list-title">{t("app.sources.panel.entities")}</h3>
            <div className="so-pill-row">{entities.slice(0, 12).map((entity) => <span key={entity.id} className="so-chip">{entity.name}</span>)}</div>
          </div>
        ) : null;
      })()}
    </Card>
  );
}
