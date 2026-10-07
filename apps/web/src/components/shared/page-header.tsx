import { Button } from "../ui/button";

export type PageAction = {
  id: string;
  label: string;
  emphasis: "primary" | "secondary";
  disabled?: boolean;
  disabledReason?: string;
  onClick?: () => void;
};

export function PageHeader({
  title,
  description,
  version,
  actions = [],
}: {
  title: string;
  description?: string;
  version?: string | null;
  actions?: readonly PageAction[];
}) {
  let primaryUsed = false;
  return (
    <header className="oo-stack">
      <div>
        <h1 className="oo-title">{title}</h1>
        {description ? <p className="oo-body">{description}</p> : null}
        {version ? <p className="oo-secondary">版本 {version}</p> : <p className="oo-secondary">版本 未知</p>}
      </div>
      {actions.length > 0 ? (
        <div className="oo-actions">
          {actions.map((action) => {
            const primary = action.emphasis === "primary" && !primaryUsed;
            if (action.emphasis === "primary") {
              primaryUsed = true;
            }
            const demoted = action.emphasis === "primary" && !primary;
            return (
              <Button
                key={action.id}
                variant={primary ? "default" : "secondary"}
                disabled={action.disabled || demoted}
                disabledReason={demoted ? "只能有一个主要动作。" : action.disabledReason}
                onClick={action.onClick}
              >
                {action.label}
              </Button>
            );
          })}
        </div>
      ) : null}
    </header>
  );
}
