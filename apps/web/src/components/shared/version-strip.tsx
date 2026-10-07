import { useState } from "react";

export type VersionTuple = {
  project: string | null;
  environment: string | null;
  catalog: string | null;
  policy: string | null;
  knowledge: string | null;
  fresh: boolean;
};

function show(value: string | null): string {
  return value && value.length > 0 ? value : "未知";
}

export function VersionStrip({ value, state = "ready" }: { value: VersionTuple; state?: "ready" | "loading" | "error" | "forbidden" }) {
  const [open, setOpen] = useState(false);
  if (state === "forbidden") return <p>当前身份没有执行此操作的权限。</p>;
  if (state === "error") return <p>版本没有读到。</p>;
  if (state === "loading") return <p aria-busy="true">正在读取版本。</p>;
  return (
    <section className="oo-panel">
      <p>{value.fresh ? "版本是当前的。" : "版本可能已经过时。"}</p>
      <button type="button" className="oo-button oo-button-ghost oo-button-sm" onClick={() => setOpen((item) => !item)}>
        {open ? "收起版本" : "查看版本"}
      </button>
      {open ? (
        <dl className="oo-code">
          <div>project {show(value.project)}</div>
          <div>environment {show(value.environment)}</div>
          <div>catalog {show(value.catalog)}</div>
          <div>policy {show(value.policy)}</div>
          <div>knowledge {show(value.knowledge)}</div>
        </dl>
      ) : null}
    </section>
  );
}
