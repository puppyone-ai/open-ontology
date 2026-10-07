import { useState } from "react";
import { TextField } from "../ui/text-field";
import { Button } from "../ui/button";
import { NOT_CONNECTED } from "./copy";

export type WizardField = {
  id: string;
  label: string;
  kind: "text" | "secret";
};

export type WizardStep = {
  id: string;
  title: string;
  fields: readonly WizardField[];
};

export function Wizard({
  steps,
  planVersion,
  canSubmit,
  onPlanInvalidated,
  onSubmit,
}: {
  steps: readonly WizardStep[];
  planVersion: string | null;
  canSubmit: boolean;
  onPlanInvalidated: () => void;
  onSubmit: () => void;
}) {
  const [index, setIndex] = useState(0);
  const [values, setValues] = useState<Record<string, string>>({});
  const [stale, setStale] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const step = steps[index];
  if (!step) {
    return <p>没有步骤。</p>;
  }
  const dirty = Object.values(values).some((value) => value.length > 0);

  function update(id: string, value: string) {
    setValues((current) => ({ ...current, [id]: value }));
    setStale(true);
    onPlanInvalidated();
  }

  return (
    <section className="oo-stack">
      <p>
        步骤 {index + 1} / {steps.length}：{step.title}
      </p>
      <p className="oo-code">plan {planVersion ?? "没有计划"}</p>
      {stale ? <p>输入已变化，旧计划失效。</p> : null}
      {step.fields.map((field) => (
        <TextField
          key={field.id}
          label={field.label}
          type={field.kind === "secret" ? "password" : "text"}
          autoComplete={field.kind === "secret" ? "new-password" : "off"}
          value={values[field.id] ?? ""}
          onChange={(event) => update(field.id, event.target.value)}
        />
      ))}
      <div className="oo-actions">
        <Button type="button" variant="secondary" disabled={index === 0} onClick={() => setIndex((value) => value - 1)}>
          上一步
        </Button>
        <Button
          type="button"
          variant="secondary"
          disabled={index >= steps.length - 1}
          onClick={() => setIndex((value) => value + 1)}
        >
          下一步
        </Button>
        <Button type="button" disabled={!canSubmit || stale || planVersion === null} disabledReason={NOT_CONNECTED} onClick={onSubmit}>
          提交
        </Button>
        <Button type="button" variant="ghost" onClick={() => (dirty ? setLeaving(true) : setValues({}))}>
          离开
        </Button>
      </div>
      {leaving ? (
        <div>
          <p>还有未提交的输入。Secret 不会留在本地。</p>
          <Button
            type="button"
            variant="destructive"
            onClick={() => {
              setValues({});
              setLeaving(false);
            }}
          >
            放弃输入
          </Button>
        </div>
      ) : null}
    </section>
  );
}
