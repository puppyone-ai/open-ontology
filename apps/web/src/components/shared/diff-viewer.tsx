export type DiffRow = {
  id: string;
  label: string;
  before: string;
  after: string;
};

export function DiffViewer({ rows, state = "ready" }: { rows: readonly DiffRow[]; state?: "ready" | "loading" | "error" | "forbidden" | "empty" }) {
  if (state === "loading") return <p aria-busy="true">正在读取差异。</p>;
  if (state === "error") return <p>差异没有读到。</p>;
  if (state === "forbidden") return <p>当前身份没有执行此操作的权限。</p>;
  if (state === "empty" || rows.length === 0) return <p>没有可显示的差异。</p>;
  return (
    <div className="oo-stack">
      {rows.map((row) => (
        <section key={row.id}>
          <h2 className="oo-section-title">{row.label}</h2>
          <div className="oo-split">
            <p>
              旧 <span className="oo-exact">{row.before}</span>
            </p>
            <p>
              新 <span className="oo-exact">{row.after}</span>
            </p>
          </div>
        </section>
      ))}
    </div>
  );
}
