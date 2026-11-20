import { useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, TextField } from "@open-ontology/design-system";
import { Check, X } from "lucide-react";
import type { ReviewItem } from "../../../shared/super-ontology/docs-api.generated";
import { subjectView } from "../../model/review/reviewModel";

type Decide = (item: ReviewItem, decision: "accept" | "reject", edit?: { name: string }) => Promise<void>;

/**
 * "Who is Speaker 1?" (D74): the recording is already in the ontology under a provisional speaker, so this asks for a name
 * instead of holding anything back. A candidate (the roster, a self-introduction) is one click; any other name can be typed;
 * "Not anyone known" keeps the provisional speaker. Confirming re-attributes what the speaker said.
 */
export function SpeakerReviewRow({ item, drive, busy, onDecide }: { item: ReviewItem; drive: string; busy: string | null; onDecide: Decide }) {
  const { t } = useLocalization();
  const view = subjectView(item);
  const [typed, setTyped] = useState("");
  if (view.kind !== "speaker") return null;
  const names = [...new Set(view.candidates.map((candidate) => candidate.name))].slice(0, 3);
  const quote = view.candidates.find((candidate) => candidate.quote)?.quote ?? item.evidence?.[0]?.quote;
  const disabled = busy !== null && busy !== item.id;
  const name = typed.trim();
  return (
    <div className="so-list-row so-speaker-row" data-testid="review-row" data-kind="speaker_identity">
      <span className="so-list-text">
        <span className="so-list-title so-truncate">{t("super-drive.review.speaker.intro", { speaker: view.speaker.replace(/^S(?=\d+$)/, "") })}</span>
        <span className="so-list-sub so-truncate">{t("super-drive.review.speaker.unconfirmed")}{item.source?.path ? ` · ${item.source.path}` : ""} · {drive}</span>
        {quote && <span className="so-quote so-truncate">{quote}</span>}
        {names.length === 0 && <span className="so-weak">{t("super-drive.review.speaker.noCandidates")}</span>}
        <span className="so-row so-speaker-actions" data-wrap>
          {names.map((candidate) => (
            <Button key={candidate} size="compact" variant="primary" icon={Check} loading={busy === item.id} disabled={disabled} data-testid="speaker-confirm-candidate" onClick={() => { void onDecide(item, "accept", { name: candidate }); }}>{t("super-drive.review.speaker.confirmAs", { name: candidate })}</Button>
          ))}
          <TextField labelHidden label={t("super-drive.review.speaker.otherName")} placeholder={t("super-drive.review.speaker.otherName")} value={typed} maxLength={120} spellCheck={false} data-testid="speaker-name" onChange={(event) => setTyped(event.target.value)} />
          <Button size="compact" icon={Check} disabled={disabled || name === ""} data-testid="speaker-confirm-typed" onClick={() => { void onDecide(item, "accept", { name }); }}>{t("super-drive.review.speaker.confirm")}</Button>
          <Button size="compact" icon={X} disabled={disabled} data-testid="speaker-none" onClick={() => { void onDecide(item, "reject"); }}>{t("super-drive.review.speaker.none")}</Button>
        </span>
      </span>
    </div>
  );
}
