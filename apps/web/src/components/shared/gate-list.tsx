import { StatusBadge } from "./status-badge";

export type GateState = "pass" | "block" | "warn" | "not_run";

export type GateItem = {
  id: string;
  state: GateState;
  summary: string;
  reportHash: string | null;
};

export function GateList({ items }: { items: readonly GateItem[] }) {
  return (
    <ol className="oo-stack">
      {items.map((item) => (
        <li key={item.id} data-gate={item.state}>
          <StatusBadge kind={item.state} />
          <span>
            {item.id} {item.summary}
          </span>
          <span className="oo-code">{item.reportHash ?? "没有报告"}</span>
        </li>
      ))}
    </ol>
  );
}
