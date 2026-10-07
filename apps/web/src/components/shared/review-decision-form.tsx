import { useEffect, useState } from "react";
import { TextArea } from "../ui/text-field";
import { Button } from "../ui/button";

export type ReviewDecision = "accept" | "edit_accept" | "reject" | "defer";

export type ReviewDraft = {
  decision: ReviewDecision;
  reason: string;
  candidateId: string;
  revision: string;
};

const LABEL: Record<ReviewDecision, string> = {
  accept: "接受",
  edit_accept: "改后接受",
  reject: "拒绝",
  defer: "暂缓",
};

export function ReviewDecisionForm({
  candidateId,
  revision,
  stale = false,
  conflict = null,
  disabled = false,
  readOnly = false,
  onSubmit,
}: {
  candidateId: string;
  revision: string;
  stale?: boolean;
  conflict?: { previous: string; next: string } | null;
  disabled?: boolean;
  readOnly?: boolean;
  onSubmit: (draft: ReviewDraft) => void;
}) {
  const [decision, setDecision] = useState<ReviewDecision | null>(null);
  const [reason, setReason] = useState("");
  const blocked = stale || conflict !== null;
  const [acknowledged, setAcknowledged] = useState(!blocked);

  useEffect(() => {
    if (blocked) {
      setDecision(null);
      setAcknowledged(false);
    }
  }, [blocked, conflict?.previous, conflict?.next, stale]);

  const needsReason = decision === "reject" || decision === "defer" || decision === "edit_accept";
  const canSubmit = !disabled && !readOnly && decision !== null && acknowledged && (!needsReason || reason.trim().length > 0);

  return (
    <form
      className="oo-stack"
      onSubmit={(event) => {
        event.preventDefault();
        if (!canSubmit || decision === null) {
          return;
        }
        onSubmit({ decision, reason, candidateId, revision });
      }}
    >
      <p className="oo-code">
        {candidateId} · {revision}
      </p>
      {stale ? <p>证据已变化，需要重新决定。</p> : null}
      {conflict ? (
        <div className="oo-split">
          <p>旧：{conflict.previous}</p>
          <p>新：{conflict.next}</p>
        </div>
      ) : null}
      <fieldset className="oo-choices" disabled={disabled || readOnly}>
        <legend>决定</legend>
        {(Object.keys(LABEL) as ReviewDecision[]).map((item) => (
          <label key={item}>
            <input
              type="radio"
              name="review-decision"
              value={item}
              checked={decision === item}
              onChange={() => {
                setDecision(item);
                setAcknowledged(true);
              }}
            />
            {LABEL[item]}
          </label>
        ))}
      </fieldset>
      <TextArea label="理由" value={reason} onChange={(event) => setReason(event.target.value)} readOnly={readOnly} />
      <Button
        type="submit"
        disabled={!canSubmit}
        disabledReason={
          readOnly ? "当前只能查看。" : disabled ? "当前不能提交。" : !canSubmit ? "请选择决定。拒绝、暂缓和改后接受需要理由。" : undefined
        }
      >
        提交决定
      </Button>
    </form>
  );
}
