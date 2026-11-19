import { forwardRef, useId, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes, type TextareaHTMLAttributes } from "react";
import { ChevronDown } from "lucide-react";
import { Icon } from "../icons/Icon";
import { cx } from "./cx";

type FieldShell = { label: string; helper?: string; error?: string | null; optionalLabel?: string; className?: string };

/** Wires label, helper and error text to a control: `error` replaces `helper` and is announced. */
function useField(helper?: string, error?: string | null) {
  const id = useId();
  const helperId = helper ? `${id}-help` : undefined;
  const errorId = error ? `${id}-err` : undefined;
  return { id, helperId, errorId, describedBy: [errorId, helperId].filter(Boolean).join(" ") || undefined };
}

function FieldFrame({ id, label, helper, error, helperId, errorId, optionalLabel, className, children }: FieldShell & { id: string; helperId?: string; errorId?: string; children: ReactNode }) {
  return (
    <div className={cx("so-field", className)} data-invalid={error ? "true" : undefined}>
      <label className="so-field-label" htmlFor={id}>
        {label}
        {optionalLabel && <span className="so-field-optional">{optionalLabel}</span>}
      </label>
      {children}
      {error ? <p id={errorId} className="so-field-error" role="alert">{error}</p> : helper ? <p id={helperId} className="so-field-helper">{helper}</p> : null}
    </div>
  );
}

export const TextField = forwardRef<HTMLInputElement, FieldShell & Omit<InputHTMLAttributes<HTMLInputElement>, "className">>(function TextField(
  { label, helper, error, optionalLabel, className, ...rest }, ref,
) {
  const f = useField(helper, error);
  return (
    <FieldFrame id={f.id} label={label} helper={helper} error={error} helperId={f.helperId} errorId={f.errorId} optionalLabel={optionalLabel} className={className}>
      <input ref={ref} id={f.id} className="so-input" aria-invalid={error ? true : undefined} aria-describedby={f.describedBy} {...rest} />
    </FieldFrame>
  );
});

export const TextArea = forwardRef<HTMLTextAreaElement, FieldShell & Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "className">>(function TextArea(
  { label, helper, error, optionalLabel, className, rows = 3, ...rest }, ref,
) {
  const f = useField(helper, error);
  return (
    <FieldFrame id={f.id} label={label} helper={helper} error={error} helperId={f.helperId} errorId={f.errorId} optionalLabel={optionalLabel} className={className}>
      <textarea ref={ref} id={f.id} rows={rows} className="so-input so-textarea" aria-invalid={error ? true : undefined} aria-describedby={f.describedBy} {...rest} />
    </FieldFrame>
  );
});

export function Select({ label, helper, error, optionalLabel, className, children, ...rest }: FieldShell & Omit<SelectHTMLAttributes<HTMLSelectElement>, "className">) {
  const f = useField(helper, error);
  return (
    <FieldFrame id={f.id} label={label} helper={helper} error={error} helperId={f.helperId} errorId={f.errorId} optionalLabel={optionalLabel} className={className}>
      <span className="so-select-wrap">
        <select id={f.id} className="so-input so-select" aria-invalid={error ? true : undefined} aria-describedby={f.describedBy} {...rest}>{children}</select>
        <Icon icon={ChevronDown} className="so-select-chevron" />
      </span>
    </FieldFrame>
  );
}

export function Switch({ checked, onChange, label, description, disabled }: { checked: boolean; onChange: (next: boolean) => void; label: string; description?: string; disabled?: boolean }) {
  const id = useId();
  return (
    <div className="so-switch-row">
      <span className="so-switch-text">
        <span id={`${id}-l`} className="so-switch-label">{label}</span>
        {description && <span id={`${id}-d`} className="so-switch-description">{description}</span>}
      </span>
      <button type="button" role="switch" className="so-switch" aria-checked={checked} aria-labelledby={`${id}-l`} aria-describedby={description ? `${id}-d` : undefined} disabled={disabled} onClick={() => onChange(!checked)}>
        <span className="so-switch-thumb" />
      </button>
    </div>
  );
}

export function Checkbox({ checked, onChange, label, disabled }: { checked: boolean; onChange: (next: boolean) => void; label: ReactNode; disabled?: boolean }) {
  return (
    <label className="so-checkbox" data-disabled={disabled ? "true" : undefined}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(event) => onChange(event.target.checked)} />
      <span className="so-checkbox-box" aria-hidden="true">
        <svg viewBox="0 0 16 16" width="16" height="16"><path d="M4 8.5l2.6 2.6L12 5.6" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" /></svg>
      </span>
      <span className="so-checkbox-label">{label}</span>
    </label>
  );
}

export function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: ReadonlyArray<{ value: T; label: string }>; onChange: (value: T) => void; label: string }) {
  return (
    <div className="so-segmented" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button key={option.value} type="button" role="radio" aria-checked={option.value === value} tabIndex={option.value === value ? 0 : -1} className="so-segmented-item" onClick={() => onChange(option.value)}
          onKeyDown={(event) => {
            const i = options.findIndex((o) => o.value === value);
            const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
            if (!step) return;
            event.preventDefault();
            const next = options[(i + step + options.length) % options.length];
            if (next) { onChange(next.value); (event.currentTarget.parentElement?.querySelectorAll<HTMLElement>("[role='radio']")[(i + step + options.length) % options.length])?.focus(); }
          }}>
          {option.label}
        </button>
      ))}
    </div>
  );
}
