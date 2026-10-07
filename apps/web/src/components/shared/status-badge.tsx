export type StatusKind =
  | "pass"
  | "block"
  | "warn"
  | "not_run"
  | "pending"
  | "unknown"
  | "loading"
  | "disabled"
  | "read_only"
  | "conflict"
  | "error"
  | "forbidden";

const LABEL: Record<StatusKind, string> = {
  pass: "通过",
  block: "阻断",
  warn: "警告",
  not_run: "未运行",
  pending: "待处理",
  unknown: "不兼容",
  loading: "进行中",
  disabled: "不可用",
  read_only: "只读",
  conflict: "冲突",
  error: "错误",
  forbidden: "没有权限",
};

const TONE: Record<StatusKind, "pass" | "warn" | "block" | "neutral"> = {
  pass: "pass",
  block: "block",
  warn: "warn",
  not_run: "neutral",
  pending: "neutral",
  unknown: "neutral",
  loading: "neutral",
  disabled: "neutral",
  read_only: "neutral",
  conflict: "warn",
  error: "block",
  forbidden: "block",
};

export function StatusBadge({ kind, reason }: { kind: StatusKind; reason?: string }) {
  return (
    <span className="oo-badge" data-kind={kind} data-tone={TONE[kind]}>
      {LABEL[kind]}
      {reason ? `：${reason}` : ""}
    </span>
  );
}
