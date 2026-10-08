import { useState } from "react";
import { TextField } from "../ui/text-field";
import { Button } from "../ui/button";
import { NOT_CONNECTED, NO_PERMISSION, dependencyText } from "./copy";

/** unbound is not forbidden. Omit the prop to keep the previous canWrite mapping. */
export type SecretConnection = "unbound" | "forbidden" | "ready";

export function SecretInput({
  configured,
  refLabel,
  canWrite,
  connection,
  onReplace,
  onClear,
}: {
  configured: boolean;
  refLabel: string | null;
  canWrite: boolean;
  connection?: SecretConnection;
  onReplace: (value: string) => void;
  onClear: () => void;
}) {
  const resolved: SecretConnection = connection ?? (canWrite ? "ready" : "forbidden");
  const writable = resolved === "ready" && canWrite;
  const blockedReason =
    resolved === "unbound" ? NOT_CONNECTED : connection === "forbidden" ? NO_PERMISSION : "当前身份没有写入权限。";
  const [value, setValue] = useState("");
  const [notice, setNotice] = useState("");
  return (
    <form
      className="oo-stack"
      autoComplete="off"
      onSubmit={(event) => {
        event.preventDefault();
        if (!writable) {
          return;
        }
        if (value.length === 0) {
          setNotice("空输入不会覆盖已有值。");
          return;
        }
        onReplace(value);
        setValue("");
        setNotice("已提交替换。输入已清空，页面不回读这个值。");
      }}
    >
      <p>{configured ? `已配置 ${refLabel ?? "引用"}` : "尚未配置"}</p>
      {resolved === "unbound" ? (
        <p>
          {NOT_CONNECTED}
          {dependencyText("B07")}
        </p>
      ) : null}
      <TextField
        label="新的 Secret"
        type="password"
        name="oo-secret-value"
        autoComplete="new-password"
        value={value}
        disabled={!writable}
        hint={writable ? "只写入新值。空输入保持原样。" : blockedReason}
        onChange={(event) => setValue(event.target.value)}
      />
      <div className="oo-actions">
        <Button type="submit" disabled={!writable} disabledReason={writable ? undefined : blockedReason}>
          替换
        </Button>
        <Button
          type="button"
          variant="secondary"
          disabled={!writable || !configured}
          disabledReason={!writable ? blockedReason : !configured ? "没有可清除的配置。" : undefined}
          onClick={() => {
            if (!writable) return;
            setValue("");
            onClear();
            setNotice("已请求清除。");
          }}
        >
          清除
        </Button>
      </div>
      {notice ? <p className="oo-help">{notice}</p> : null}
    </form>
  );
}
