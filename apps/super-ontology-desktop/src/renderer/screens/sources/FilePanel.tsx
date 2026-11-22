import { useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, DefinitionList, IconButton, Notice, Skeleton, StatusDot } from "@open-ontology/design-system";
import { RotateCw, X } from "lucide-react";
import { ErrorText } from "../shared/ErrorText";
import { useFileDetail } from "../../model/details/useFileDetail";
import { formatBytes } from "../../model/format";
import { useSuperDriveActions, useSuperDriveSelector } from "../../model/SuperDriveProvider";
import type { SuperDriveError } from "../../model/superDriveErrors";
import { profileOf, reasonOf, useClassText, useProfileText } from "../../model/profile";
import { setProfileOverride } from "../../model/profileOverride";
import { AUTO_PROFILE, ProfileSelect } from "./ProfileSelect";
import { fileView, TONE, useFileStateText } from "./fileView";

/** What the engine made of one file: state and why, size, and the entities it contributed to. Retry is offered when it can help. */
export function FilePanel({ fileId, onClose }: { fileId: string; onClose: () => void }) {
  const t$ = useLocalization();
  const { t, formatDate } = t$;
  const actions = useSuperDriveActions();
  const stateText = useFileStateText();
  const profileText = useProfileText();
  const classText = useClassText();
  const record = useSuperDriveSelector((s) => s.files[fileId] ?? null);
  const { detail, error, loading } = useFileDetail(fileId);
  const [retrying, setRetrying] = useState(false);
  const [classifying, setClassifying] = useState(false);
  const [classError, setClassError] = useState<SuperDriveError | null>(null);
  /** The engine reclassifies the file and extracts it again (it goes outdated and back); the record follows by events. */
  const classify = async (profile: Parameters<typeof setProfileOverride>[0]["profile"]) => {
    if (!record) return;
    setClassifying(true); setClassError(null);
    const result = await setProfileOverride({ driveId: record.drive_id, scope: "file", path: record.path, profile });
    if (!result.ok) setClassError(result.error);
    setClassifying(false);
  };
  const view = record ? fileView(record) : null;
  const retry = async () => { setRetrying(true); await actions.retryFiles([fileId]); setRetrying(false); };

  return (
    <Card className="so-form" aria-label={t("app.sources.panel.title")} role="complementary" data-testid="file-detail-panel" style={{ position: "sticky", top: 0 }}>
      <div className="so-row">
        <h2 className="so-section-title so-grow so-truncate">{record?.name ?? record?.path ?? <Skeleton width={160} height={16} />}</h2>
        <IconButton icon={X} label={t("app.common.close")} size="compact" onClick={onClose} />
      </div>
      {view && <StatusDot tone={TONE[view.tone]} label={stateText(view)} pulse={view.busy} />}
      {view?.canRetry && <div><Button icon={RotateCw} loading={retrying} onClick={() => { void retry(); }}>{t("app.sources.panel.retry")}</Button></div>}
      {error && <Notice tone="danger" title={t("app.common.loadFailed")}><ErrorText error={error} /></Notice>}
      {record && (
        <DefinitionList items={[
          { label: t("app.sources.panel.path"), value: <span className="so-mono">{record.path}</span> },
          { label: t("app.sources.col.size"), value: formatBytes(record.size, t$) || "–" },
          { label: t("app.sources.col.modified"), value: record.modified_at ? formatDate(new Date(record.modified_at), { dateStyle: "medium", timeStyle: "short" }) : "–" },
          { label: t("app.sources.panel.profile"), value: <span data-testid="file-detail-profile">{profileText(record)}</span> },
          { label: t("app.sources.panel.class"), value: <span data-testid="file-detail-class">{classText.name(record)}</span> },
          ...(record.format ? [{ label: t("app.sources.panel.format"), value: record.format }] : []),
        ]} />
      )}
      {record && (
        <div className="so-stack">
          <p className="so-weak" data-testid="file-class-line">{classText.line(record)}</p>
          <ProfileSelect label={t("app.sources.override.label")} value={reasonOf(record) === "user" ? profileOf(record) : AUTO_PROFILE} disabled={classifying} testId="file-profile-select" onChange={(profile) => { void classify(profile); }} />
          {classError && <Notice tone="danger" title={t("app.sources.override.failed")}><ErrorText error={classError} /></Notice>}
        </div>
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
