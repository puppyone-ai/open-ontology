import { useLocalization } from "@open-ontology/design-system/localization";
import { Check, Copy } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import "./share-parts.css";

export function ScopeOption({
  checked,
  icon,
  label,
  onSelect,
}: {
  checked: boolean;
  icon: ReactNode;
  label: ReactNode;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      className={`desktop-share-scope-option${checked ? " is-selected" : ""}`}
      onClick={onSelect}
    >
      <span className="desktop-share-scope-option-icon" aria-hidden="true">{icon}</span>
      <span className="desktop-share-scope-option-label">{label}</span>
      {checked && <Check size={13} strokeWidth={2.2} aria-hidden="true" />}
    </button>
  );
}

export function CopyField({
  label,
  value,
  display,
  disabled = false,
  multiline = false,
  trailing = null,
}: {
  label: string;
  value: string;
  display: string;
  disabled?: boolean;
  multiline?: boolean;
  trailing?: ReactNode;
}) {
  const { t } = useLocalization();
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return undefined;
    const timer = window.setTimeout(() => setCopied(false), 1600);
    return () => window.clearTimeout(timer);
  }, [copied]);
  return (
    <div className={`desktop-share-copy-field${multiline ? " is-multiline" : ""}`}>
      <span className="desktop-share-copy-label">{label}</span>
      <code className="desktop-share-copy-value" dir="ltr">{display}</code>
      <span className="desktop-share-copy-actions">
        {trailing}
        <button
          type="button"
          className={`desktop-share-copy-button${copied ? " is-copied" : ""}`}
          disabled={disabled || !value}
          aria-label={t("super-drive.share.field.copyLabel", { label })}
          onClick={() => {
            void navigator.clipboard.writeText(value).then(() => setCopied(true)).catch(() => {});
          }}
        >
          {copied ? <Check size={13} strokeWidth={2.2} /> : <Copy size={13} />}
          <span>{t(copied ? "super-drive.share.field.copied" : "super-drive.share.field.copy")}</span>
        </button>
      </span>
    </div>
  );
}
