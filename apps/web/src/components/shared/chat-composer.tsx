import { useRef, useState, type KeyboardEvent } from "react";
import { TextArea } from "../ui/text-field";
import { Button } from "../ui/button";
import { NO_PERMISSION } from "./copy";

export type ComposerState = "ready" | "sending" | "streaming" | "disabled" | "readOnly" | "forbidden" | "error";

export function ChatComposer({
  state,
  disabledReason,
  onSend,
  onStop,
  onEndSession,
}: {
  state: ComposerState;
  disabledReason?: string;
  onSend: (text: string) => void;
  onStop: () => void;
  onEndSession: () => void;
}) {
  const [text, setText] = useState("");
  const composing = useRef(false);
  const locked = state === "sending" || state === "streaming" || state === "disabled" || state === "readOnly" || state === "forbidden" || state === "error";
  const reason = state === "forbidden" ? NO_PERMISSION : disabledReason;

  function send() {
    const value = text.trim();
    if (locked || value.length === 0) {
      return;
    }
    onSend(value);
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key !== "Enter" || event.shiftKey) {
      return;
    }
    if (event.nativeEvent.isComposing || composing.current) {
      return;
    }
    event.preventDefault();
    send();
  }

  return (
    <form
      className="oo-stack"
      onSubmit={(event) => {
        event.preventDefault();
        send();
      }}
    >
      <TextArea
        label="问题"
        value={text}
        readOnly={state === "readOnly"}
        disabled={state === "disabled" || state === "forbidden"}
        hint={reason}
        onChange={(event) => setText(event.target.value)}
        onCompositionStart={() => {
          composing.current = true;
        }}
        onCompositionEnd={() => {
          composing.current = false;
        }}
        onKeyDown={onKeyDown}
      />
      <div className="oo-actions">
        <Button type="submit" disabled={locked || text.trim().length === 0} loading={state === "sending"} disabledReason={reason}>
          发送
        </Button>
        <Button type="button" variant="secondary" disabled={state !== "streaming" && state !== "sending"} onClick={onStop}>
          停止本轮
        </Button>
        <Button type="button" variant="ghost" disabled={state === "disabled" || state === "forbidden"} disabledReason={reason} onClick={onEndSession}>
          结束会话
        </Button>
      </div>
    </form>
  );
}
