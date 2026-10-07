import { useState } from "react";
import { TextField } from "../ui/text-field";
import { Button } from "../ui/button";

export function SecretInput({
  configured,
  refLabel,
  canWrite,
  onReplace,
  onClear,
}: {
  configured: boolean;
  refLabel: string | null;
  canWrite: boolean;
  onReplace: (value: string) => void;
  onClear: () => void;
}) {
  const [value, setValue] = useState("");
  const [notice, setNotice] = useState("");
  return (
    <form
      className="oo-stack"
      autoComplete="off"
      onSubmit={(event) => {
        event.preventDefault();
        if (!canWrite) {
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
      <TextField
        label="新的 Secret"
        type="password"
        name="oo-secret-value"
        autoComplete="new-password"
        value={value}
        disabled={!canWrite}
        hint={canWrite ? "只写入新值。空输入保持原样。" : "当前身份没有写入权限。"}
        onChange={(event) => setValue(event.target.value)}
      />
      <div className="oo-actions">
        <Button type="submit" disabled={!canWrite} disabledReason={canWrite ? undefined : "当前身份没有写入权限。"}>
          替换
        </Button>
        <Button
          type="button"
          variant="secondary"
          disabled={!canWrite || !configured}
          disabledReason={!canWrite ? "当前身份没有写入权限。" : !configured ? "没有可清除的配置。" : undefined}
          onClick={() => {
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
