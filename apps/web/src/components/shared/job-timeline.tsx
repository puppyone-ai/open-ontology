export type JobEvent = {
  id: string;
  sequence: number;
  label: string;
  state: "running" | "reconnecting" | "failed" | "confirmed" | "eof";
};

export type JobProgress =
  | { kind: "exact"; completed: number; total: number }
  | { kind: "estimated"; completed: number; total: number; denominator: string }
  | null;

export function JobTimeline({
  events,
  progress,
  state = "ready",
}: {
  events: readonly JobEvent[];
  progress: JobProgress;
  state?: "ready" | "loading" | "error" | "forbidden" | "disabled";
}) {
  if (state === "forbidden") return <p>当前身份没有执行此操作的权限。</p>;
  if (state === "error") return <p>任务状态没有读到。</p>;
  if (state === "loading") return <p aria-busy="true">正在读取任务。</p>;
  return (
    <div className="oo-stack">
      {state === "disabled" ? <p>当前没有可执行动作。</p> : null}
      <ol>
        {events.map((event) => (
          <li key={event.id}>
            <span className="oo-code">{event.sequence}</span> {event.label} · {event.state}
            {event.state === "eof" ? "。EOF 不是完成。" : ""}
          </li>
        ))}
      </ol>
      {progress?.kind === "exact" ? (
        <p>
          进度 {progress.completed}/{progress.total}
        </p>
      ) : null}
      {progress?.kind === "estimated" ? (
        <p>
          估计进度 {progress.completed}/{progress.total}。分母：{progress.denominator || "未说明"}
        </p>
      ) : null}
    </div>
  );
}
