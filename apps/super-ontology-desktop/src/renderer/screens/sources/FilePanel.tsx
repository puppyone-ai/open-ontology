import { useCallback, useEffect, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, DefinitionList, IconButton, Notice, Skeleton, StatusDot } from "@open-ontology/design-system";
import { RotateCw, X } from "lucide-react";
import { ErrorText } from "../shared/ErrorText";
import { MediaClassBadge, MediaPreview } from "../../features/media/MediaPreview";
import { useMediaController } from "../../features/media/useMediaController";
import { useMediaSource } from "../../features/media/useMediaSource";
import { mediaKindOf, speakerLetter } from "../../model/details/mediaModel";
import { SpeakerReviewGroup } from "../ontology/SpeakerReviewGroup";
import { invoke } from "../../model/bridgeInvoke";
import type { ReviewItem } from "../../../shared/super-ontology/docs-api.generated";
import { useFileDetail } from "../../model/details/useFileDetail";
import { formatBytes } from "../../model/format";
import { useSuperDriveActions, useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { fileView, TONE, useFileStateText } from "./fileView";

/** What the engine made of one file: state and why, size, and the entities it contributed to. Retry is offered when it can help. */
export function FilePanel({ fileId, onClose }: { fileId: string; onClose: () => void }) {
  const t$ = useLocalization();
  const { t, formatDate } = t$;
  const actions = useSuperDriveActions();
  const stateText = useFileStateText();
  const record = useSuperDriveSelector((s) => s.files[fileId] ?? null);
  const { detail, error, loading } = useFileDetail(fileId);
  const [retrying, setRetrying] = useState(false);
  const view = record ? fileView(record) : null;
  const isMedia = record ? mediaKindOf(typeof record.format === "string" ? record.format : null, record.path) !== null : false;
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
          ...(record.format ? [{ label: t("app.sources.panel.format"), value: record.format }] : []),
        ]} />
      )}
      {isMedia && record && <MediaSection fileId={fileId} driveId={record.drive_id} />}
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

/**
 * A recording in the file panel: what the engine took it for, the player with its transcript and, for a transcribed recording, the speakers
 * that can be named (D97, D98): nothing waits for it, so it is a list on the file and not an item of the review queue.
 */
function MediaSection({ fileId, driveId }: { fileId: string; driveId: string }) {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const controller = useMediaController();
  const source = useMediaSource(fileId);
  const info = source.state === "ready" ? source.info : null;
  const [toName, setToName] = useState<ReviewItem[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const load = useCallback(async () => {
    const result = await invoke("review-list", { driveId, kind: "speaker_identity", status: "pending", limit: 50 });
    if (result.ok) setToName((result.value.items as unknown as ReviewItem[]).filter((item) => item.source?.file_id === fileId));
  }, [driveId, fileId]);
  useEffect(() => { void load(); }, [load]);
  const decide = async (item: ReviewItem, decision: "accept" | "reject", edit?: { name: string }) => {
    setBusy(item.id);
    await actions.decideReviewItems({ driveId, items: [{ id: item.id, revision: item.revision, decision, ...(edit ? { edit } : {}) }] });
    setBusy(null);
    await load();
  };
  const name = (source.state === "ready" ? source.name : "");
  return (
    <div className="so-stack" data-testid="file-media">
      <div className="so-row">
        <h3 className="so-list-title so-grow">{t("app.media.preview.title")}</h3>
        <MediaClassBadge info={info} />
      </div>
      {info?.class === "non_conversation" && <p className="so-weak">{t("app.media.noSpeakersNote")}</p>}
      <MediaPreview source={source} controller={controller} compact speakerText={(label) => t("app.speaker.name", { letter: speakerLetter(label) })} />
      {toName.length > 0 && (
        <div className="so-stack" data-testid="speakers-to-name">
          <h3 className="so-list-title">{t("app.review.speaker.toName", { count: toName.length })}</h3>
          <p className="so-weak">{t("app.review.speaker.toNameSub")}</p>
          <SpeakerReviewGroup items={toName} fileName={name} driveName="" busy={busy} onDecide={decide} source={source} controller={controller} active onActivate={() => undefined} />
        </div>
      )}
    </div>
  );
}
