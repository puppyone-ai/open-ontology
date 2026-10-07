export type EvidenceRef = {
  id: string;
  hash: string;
  collectedAt: string;
  summary: string;
  technical?: string;
};

export function EvidencePanel({ items, state = "ready" }: { items: readonly EvidenceRef[]; state?: "ready" | "loading" | "error" | "forbidden" | "empty" }) {
  if (state === "forbidden") {
    return <p>当前身份没有执行此操作的权限。</p>;
  }
  if (state === "error") {
    return <p>证据没有读到。</p>;
  }
  if (state === "loading") {
    return <p aria-busy="true">正在读取证据。</p>;
  }
  if (state === "empty" || items.length === 0) {
    return <p>没有可显示的证据。</p>;
  }
  return (
    <div className="oo-stack">
      {items.map((item) => (
        <article key={item.id} className="oo-panel">
          <h2 className="oo-section-title">{item.summary}</h2>
          <p className="oo-code">
            {item.id} · {item.hash}
          </p>
          <p className="oo-secondary">{item.collectedAt}</p>
          {item.technical ? (
            <details>
              <summary>技术细节</summary>
              <pre className="oo-code">{item.technical}</pre>
            </details>
          ) : null}
        </article>
      ))}
    </div>
  );
}
