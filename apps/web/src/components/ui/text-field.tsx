import { useId, type InputHTMLAttributes, type TextareaHTMLAttributes } from "react";

type Common = {
  label: string;
  hint?: string;
  error?: string;
  required?: boolean;
};

export type TextFieldProps = Common & InputHTMLAttributes<HTMLInputElement>;
export type TextAreaProps = Common & TextareaHTMLAttributes<HTMLTextAreaElement>;

function described(hint: string | undefined, error: string | undefined, hintId: string, errorId: string): string | undefined {
  const ids = [hint ? hintId : "", error ? errorId : ""].filter(Boolean);
  return ids.length > 0 ? ids.join(" ") : undefined;
}

export function FieldHelp({ hint, error, hintId, errorId }: { hint?: string; error?: string; hintId: string; errorId: string }) {
  return (
    <>
      {hint ? (
        <p id={hintId} className="oo-help">
          {hint}
        </p>
      ) : null}
      {error ? (
        <p id={errorId} className="oo-help oo-help-error">
          {error}
        </p>
      ) : null}
    </>
  );
}

export function TextField({ label, hint, error, required, id, ...props }: TextFieldProps) {
  const generated = useId();
  const fieldId = id ?? generated;
  const hintId = `${fieldId}-hint`;
  const errorId = `${fieldId}-error`;
  return (
    <div className="oo-field">
      <label htmlFor={fieldId}>
        {label}
        {required ? <span aria-hidden="true"> *</span> : null}
      </label>
      <input
        {...props}
        id={fieldId}
        className="oo-input"
        required={required}
        aria-invalid={error ? true : undefined}
        aria-describedby={described(hint, error, hintId, errorId)}
        aria-required={required || undefined}
      />
      <FieldHelp hint={hint} error={error} hintId={hintId} errorId={errorId} />
    </div>
  );
}

export function TextArea({ label, hint, error, required, id, ...props }: TextAreaProps) {
  const generated = useId();
  const fieldId = id ?? generated;
  const hintId = `${fieldId}-hint`;
  const errorId = `${fieldId}-error`;
  return (
    <div className="oo-field">
      <label htmlFor={fieldId}>
        {label}
        {required ? <span aria-hidden="true"> *</span> : null}
      </label>
      <textarea
        {...props}
        id={fieldId}
        className="oo-input"
        required={required}
        aria-invalid={error ? true : undefined}
        aria-describedby={described(hint, error, hintId, errorId)}
        aria-required={required || undefined}
      />
      <FieldHelp hint={hint} error={error} hintId={hintId} errorId={errorId} />
    </div>
  );
}
