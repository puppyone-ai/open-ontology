import { useEffect, useMemo, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Badge, Button, Card, EmptyState, Notice, Skeleton } from "@open-ontology/design-system";
import { Check, ListChecks, X } from "lucide-react";
import { ErrorText } from "../shared/ErrorText";
import { useDisplayName } from "../../hooks/useDisplayName";
import { SpeakerReviewGroup } from "./SpeakerReviewGroup";
import { MediaPreview } from "../../features/media/MediaPreview";
import { useMediaController } from "../../features/media/useMediaController";
import { useMediaSource } from "../../features/media/useMediaSource";
import { representativeClips, speakerLetter } from "../../model/details/mediaModel";
import { presentReviewKind, subjectView } from "../../model/review/reviewModel";
import { useSuperDriveActions, useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { selectDrives } from "../../model/superDriveSelectors";
import { reviewKey } from "../../model/superDriveState";
import type { ReviewItem } from "../../../shared/super-ontology/docs-api.generated";
import type { SuperDriveError } from "../../model/superDriveErrors";

function describe(item: ReviewItem, t: ReturnType<typeof useLocalization>["t"], shown: (name: string) => string): { title: string; detail: string } {
  const view = subjectView(item);
  if (view.kind === "fact") return { title: [view.subject ? shown(view.subject.name) : undefined, view.predicate.replace(/_/g, " "), view.object ? shown(view.object.name) : String(Object.values(view.value ?? {})[0] ?? "")].filter(Boolean).join(" · "), detail: t(presentReviewKind(item.kind).messageId) };
  if (view.kind === "entities") return { title: view.entities.map((e) => shown(e.name)).join(" ↔ ") || t(presentReviewKind(item.kind).messageId), detail: t(presentReviewKind(item.kind).messageId) };
  return { title: t(presentReviewKind(item.kind).messageId), detail: item.source?.path ?? "" };
}

/** Things Super Ontology was not sure about. Plain accept and reject are decided here; kinds that need a choice open the engine's own flow in the CLI. */
export function ReviewQueue() {
  const { t, formatNumber } = useLocalization();
  const shown = useDisplayName();
  const actions = useSuperDriveActions();
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d === b[i]));
  const lists = useSuperDriveSelector((s) => s.review.lists);
  const withReview = drives.filter((d) => (d.pending_review ?? 0) > 0);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<SuperDriveError | null>(null);
  useEffect(() => { for (const drive of withReview) void actions.loadReviewItems({ driveId: drive.id }); }, [actions, withReview.map((d) => `${d.id}:${d.pending_review}`).join(",")]); // eslint-disable-line react-hooks/exhaustive-deps

  const rows = withReview.flatMap((drive) => (lists[reviewKey(drive.id)]?.items ?? []).filter((item) => item.status === "pending").map((item) => ({ drive, item })));
  // The speakers of one recording are asked about together, beside a player (D74 as revised): one group per recording, in the place of its first item.
  const groups = useMemo(() => {
    const byFile = new Map<string, { key: string; fileId: string | null; fileName: string; driveName: string; items: ReviewItem[] }>();
    for (const { drive, item } of rows) {
      if (item.kind !== "speaker_identity") continue;
      const view = subjectView(item);
      const key = `${drive.id}:${view.kind === "speaker" && view.blobHash ? view.blobHash : item.id}`;
      const path = item.source?.path ?? "";
      const group = byFile.get(key) ?? { key, fileId: item.source?.file_id ?? null, fileName: path.slice(path.lastIndexOf("/") + 1), driveName: drive.name, items: [] };
      group.items.push(item);
      byFile.set(key, group);
    }
    return byFile;
  }, [rows.map(({ item }) => item.id).join(",")]); // eslint-disable-line react-hooks/exhaustive-deps
  const [activeKey, setActiveKey] = useState<string | null>(null);
  const active = (activeKey && groups.get(activeKey)) || [...groups.values()][0] || null;
  const controller = useMediaController();
  const source = useMediaSource(active?.fileId ?? null);
  const [listenTo, setListenTo] = useState<{ key: string; speaker?: string } | null>(null);
  // A speaker's play button on a recording that is not the one shown: show it, then play as soon as it has loaded.
  useEffect(() => {
    if (!listenTo || !active || active.key !== listenTo.key || source.state !== "ready") return;
    setListenTo(null);
    if (!listenTo.speaker) return;
    const clips = representativeClips(source.utterances, listenTo.speaker);
    if (clips.length > 0) controller.playQueue(clips, listenTo.speaker);
  }, [listenTo, active, source, controller]);
  const speakerText = (label: string) => t("app.speaker.name", { letter: speakerLetter(label) });
  const loading = withReview.some((d) => !lists[reviewKey(d.id)]?.loaded);
  const decide = async (driveId: string, item: ReviewItem, decision: "accept" | "reject", edit?: { name: string }) => {
    setBusy(item.id); setError(null);
    const result = await actions.decideReviewItems({ driveId, items: [{ id: item.id, revision: item.revision, decision, ...(edit ? { edit } : {}) }] });
    setBusy(null);
    if (!result.ok) setError(result.error);
  };

  if (withReview.length === 0) return <Card><EmptyState icon={ListChecks} title={t("app.review.empty.title")} description={t("app.review.empty.body")} /></Card>;
  return (
    <div className="so-stack">
      {error && <Notice tone="danger" title={t("app.review.failed")} action={<Button size="compact" onClick={() => setError(null)}>{t("app.common.close")}</Button>}><ErrorText error={error} /></Notice>}
      <p className="so-muted">{t("app.review.intro")}</p>
      {loading && rows.length === 0 ? <Card className="so-list" aria-busy="true">{Array.from({ length: 3 }, (_, i) => <div key={i} className="so-list-row"><Skeleton width="60%" height={13} /></div>)}</Card> : (
        <div className="so-review-layout" data-pane={groups.size > 0 ? "true" : undefined}>
          <Card className="so-list" data-testid="review-list">
            {rows.map(({ drive, item }) => {
              if (item.kind === "speaker_identity") {
                const view = subjectView(item);
                const key = `${drive.id}:${view.kind === "speaker" && view.blobHash ? view.blobHash : item.id}`;
                const group = groups.get(key);
                if (!group || group.items[0]?.id !== item.id) return null;
                return (
                  <SpeakerReviewGroup key={key} items={group.items} fileName={group.fileName} driveName={group.driveName} busy={busy} source={active?.key === key ? source : { state: "loading" }} controller={controller}
                    active={active?.key === key} onDecide={(it, decision, edit) => decide(drive.id, it, decision, edit)}
                    onActivate={(speaker) => { setActiveKey(key); setListenTo({ key, ...(speaker ? { speaker } : {}) }); }} />
                );
              }
              const { title, detail } = describe(item, t, shown);
              const simple = presentReviewKind(item.kind).simpleDecisions;
              return (
                <div key={item.id} className="so-list-row" data-testid="review-row">
                  <span className="so-list-text">
                    <span className="so-list-title so-truncate">{title}</span>
                    <span className="so-list-sub so-truncate">{detail}{item.source?.path ? ` · ${item.source.path}` : ""} · {drive.name}</span>
                    {item.evidence?.[0]?.quote && <span className="so-quote so-truncate">{item.evidence[0].quote}</span>}
                  </span>
                  {typeof item.confidence_bp === "number" && <Badge tone="warning">{formatNumber(item.confidence_bp / 10_000, { style: "percent" })}</Badge>}
                  {simple ? (
                    <span className="so-row">
                      <Button size="compact" icon={X} loading={busy === item.id} disabled={busy !== null && busy !== item.id} onClick={() => { void decide(drive.id, item, "reject"); }}>{t("app.review.reject")}</Button>
                      <Button size="compact" variant="primary" icon={Check} disabled={busy !== null} onClick={() => { void decide(drive.id, item, "accept"); }}>{t("app.review.accept")}</Button>
                    </span>
                  ) : <span className="so-weak">{t("app.review.cli")}</span>}
                </div>
              );
            })}
          </Card>
          {groups.size > 0 && (
            <aside className="so-review-pane" aria-label={t("app.media.preview.title")} data-testid="review-pane">
              <Card className="so-form"><MediaPreview source={source} controller={controller} speakerText={speakerText} /></Card>
            </aside>
          )}
        </div>
      )}
    </div>
  );
}
