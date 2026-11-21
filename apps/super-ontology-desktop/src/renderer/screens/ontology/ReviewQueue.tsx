import { useEffect, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Badge, Button, Card, EmptyState, Notice, Skeleton } from "@open-ontology/design-system";
import { Check, ListChecks, X } from "lucide-react";
import { ErrorText } from "../shared/ErrorText";
import { SpeakerReviewRow } from "./SpeakerReviewRow";
import { presentReviewKind, subjectView } from "../../model/review/reviewModel";
import { useSuperDriveActions, useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { selectDrives } from "../../model/superDriveSelectors";
import { reviewKey } from "../../model/superDriveState";
import type { ReviewItem } from "../../../shared/super-ontology/docs-api.generated";
import type { SuperDriveError } from "../../model/superDriveErrors";

function describe(item: ReviewItem, t: ReturnType<typeof useLocalization>["t"]): { title: string; detail: string } {
  const view = subjectView(item);
  if (view.kind === "fact") return { title: [view.subject?.name, view.predicate.replace(/_/g, " "), view.object?.name ?? String(Object.values(view.value ?? {})[0] ?? "")].filter(Boolean).join(" · "), detail: t(presentReviewKind(item.kind).messageId) };
  if (view.kind === "entities") return { title: view.entities.map((e) => e.name).join(" ↔ ") || t(presentReviewKind(item.kind).messageId), detail: t(presentReviewKind(item.kind).messageId) };
  return { title: t(presentReviewKind(item.kind).messageId), detail: item.source?.path ?? "" };
}

/** Things Super Ontology was not sure about. Plain accept and reject are decided here; kinds that need a choice open the engine's own flow in the CLI. */
export function ReviewQueue() {
  const { t, formatNumber } = useLocalization();
  const actions = useSuperDriveActions();
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d === b[i]));
  const lists = useSuperDriveSelector((s) => s.review.lists);
  const withReview = drives.filter((d) => (d.pending_review ?? 0) > 0);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<SuperDriveError | null>(null);
  useEffect(() => { for (const drive of withReview) void actions.loadReviewItems({ driveId: drive.id }); }, [actions, withReview.map((d) => `${d.id}:${d.pending_review}`).join(",")]); // eslint-disable-line react-hooks/exhaustive-deps

  const rows = withReview.flatMap((drive) => (lists[reviewKey(drive.id)]?.items ?? []).filter((item) => item.status === "pending").map((item) => ({ drive, item })));
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
        <Card className="so-list" data-testid="review-list">
          {rows.map(({ drive, item }) => {
            const { title, detail } = describe(item, t);
            const simple = presentReviewKind(item.kind).simpleDecisions;
            if (item.kind === "speaker_identity") return <SpeakerReviewRow key={item.id} item={item} drive={drive.name} busy={busy} onDecide={(it, decision, edit) => decide(drive.id, it, decision, edit)} />;
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
      )}
    </div>
  );
}
