import { dependencyText, NO_PERMISSION, NOT_CONNECTED } from "./copy";

export type StateKind =
  | "empty"
  | "noMatch"
  | "error"
  | "forbidden"
  | "unsupported"
  | "notConnected"
  | "notFound"
  | "loading"
  | "conflict"
  | "readOnly";

const COPY: Record<StateKind, string> = {
  empty: "没有可显示的记录。",
  noMatch: "没有符合当前筛选的记录。",
  error: "这一步没有完成。",
  forbidden: NO_PERMISSION,
  unsupported: "当前版本不支持这个操作。",
  notConnected: NOT_CONNECTED,
  notFound: "找不到这个页面。",
  loading: "正在读取。",
  conflict: "记录已经变化，需要重新决定。",
  readOnly: "当前只能查看。",
};

export function StatePanel({
  kind,
  dependency,
  traceId,
  detail,
}: {
  kind: StateKind;
  dependency?: string;
  traceId?: string | null;
  detail?: string;
}) {
  return (
    <section className="oo-panel" data-state={kind} aria-busy={kind === "loading" || undefined}>
      <p>{COPY[kind]}</p>
      {kind === "notConnected" && dependency ? <p className="oo-secondary">{dependencyText(dependency)}</p> : null}
      {detail ? <p className="oo-secondary">{detail}</p> : null}
      {traceId ? <p className="oo-code">trace {traceId}</p> : null}
    </section>
  );
}
