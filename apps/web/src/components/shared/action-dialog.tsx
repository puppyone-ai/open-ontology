import { useState } from "react";
import { DialogFrame } from "../ui/dialog";
import { TextArea } from "../ui/text-field";
import { Button } from "../ui/button";

export function ActionDialog({
  open,
  title,
  scope,
  impact,
  destructive,
  dirty,
  onConfirm,
  onOpenChange,
}: {
  open: boolean;
  title: string;
  scope: string;
  impact: string;
  destructive: boolean;
  dirty: boolean;
  onConfirm: (reason: string) => void;
  onOpenChange: (open: boolean) => void;
}) {
  const [reason, setReason] = useState("");
  const [discarding, setDiscarding] = useState(false);
  const needsReason = destructive;
  const canConfirm = !needsReason || reason.trim().length > 0;

  function requestClose(next: boolean) {
    if (!next && dirty && !discarding) {
      setDiscarding(true);
      return;
    }
    if (!next) {
      setDiscarding(false);
      setReason("");
    }
    onOpenChange(next);
  }

  return (
    <DialogFrame open={open} title={title} description={scope} onOpenChange={requestClose}>
      <p>影响：{impact}</p>
      {needsReason ? (
        <TextArea label="理由" value={reason} required onChange={(event) => setReason(event.target.value)} />
      ) : null}
      {discarding ? <p>还有未提交的内容。再按一次关闭才会放弃。</p> : null}
      <div className="oo-actions">
        <Button
          type="button"
          variant={destructive ? "destructive" : "default"}
          disabled={!canConfirm}
          disabledReason={canConfirm ? undefined : "需要填写理由。"}
          onClick={() => {
            onConfirm(reason);
            setReason("");
          }}
        >
          确认
        </Button>
        <Button type="button" variant="ghost" onClick={() => requestClose(false)}>
          关闭
        </Button>
      </div>
    </DialogFrame>
  );
}
