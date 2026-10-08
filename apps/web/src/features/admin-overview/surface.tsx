import { useEffect, type ReactNode } from "react";
import { NOT_CONNECTED } from "../../components/shared/copy";
import { PageHeader, type PageAction } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { AdminFrame } from "./admin-frame";
import { consume } from "./consumption";
import { legalAdminCall } from "./legal-call";

export function AdminSurface({
  title,
  description,
  groups,
  operationIds,
  action,
  notes,
  children,
}: {
  title: string;
  description: string;
  groups: readonly string[];
  operationIds: readonly string[];
  action: { id: string; label: string } | null;
  notes: readonly string[];
  children?: ReactNode;
}) {
  useEffect(() => {
    document.title = title;
  }, [title]);

  const actions: PageAction[] = action
    ? [
        {
          id: action.id,
          label: action.label,
          emphasis: "secondary",
          disabled: true,
          disabledReason: NOT_CONNECTED,
        },
      ]
    : [];

  return (
    <AdminFrame>
      <PageHeader title={title} description={description} version={null} actions={actions} />
      <p className="oo-secondary">项目、环境和版本都未知。</p>
      <StatePanel kind="notConnected" dependency={groups.join("、")} />
      <ul>
        {operationIds.map((operationId) => {
          const item = consume(operationId);
          const state = item.record?.state ?? "missing";
          const legal = legalAdminCall(operationId, null);
          return (
            <li key={operationId}>
              <span>
                {operationId} 状态 {state}，调用 {item.result.reason}
              </span>
              <span>
                {operationId} 预检 {legal.reason}
              </span>
            </li>
          );
        })}
      </ul>
      {notes.map((note) => (
        <p key={note}>{note}</p>
      ))}
      {children}
    </AdminFrame>
  );
}
