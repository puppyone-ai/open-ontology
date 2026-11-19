import { useEffect, useRef, useState, type ReactNode } from "react";
import { AlertCircle, AlertTriangle, Check, CheckCircle2, Copy, Info, type LucideIcon } from "lucide-react";
import { Icon } from "../icons/Icon";
import { Button } from "./Button";
import { cx } from "./cx";

export type Tone = "neutral" | "accent" | "success" | "warning" | "danger";

/** A state shown as a coloured dot and a word. Colour is never the only signal. */
export function StatusDot({ tone, label, pulse = false }: { tone: Tone; label: string; pulse?: boolean }) {
  return (
    <span className="so-status" data-tone={tone}>
      <span className="so-status-dot" data-pulse={pulse ? "true" : undefined} aria-hidden="true" />
      <span className="so-status-label">{label}</span>
    </span>
  );
}

/** Reserved for failures and things that need the user. */
export function Badge({ tone = "neutral", children }: { tone?: Tone; children: ReactNode }) {
  return <span className="so-badge" data-tone={tone}>{children}</span>;
}

const NOTICE_ICON: Record<Exclude<Tone, "neutral" | "accent">, LucideIcon> = { success: CheckCircle2, warning: AlertTriangle, danger: AlertCircle };

/** An inline message: what happened, why, and the next step. */
export function Notice({ tone = "accent", title, children, action, role }: { tone?: Exclude<Tone, "neutral">; title?: string; children?: ReactNode; action?: ReactNode; role?: "status" | "alert" }) {
  const glyph = tone === "accent" ? Info : NOTICE_ICON[tone];
  return (
    <div className="so-notice" data-tone={tone} role={role ?? (tone === "danger" ? "alert" : "status")}>
      <Icon icon={glyph} className="so-notice-icon" />
      <div className="so-notice-body">
        {title && <p className="so-notice-title">{title}</p>}
        {children != null && <div className="so-notice-text">{children}</div>}
      </div>
      {action && <div className="so-notice-action">{action}</div>}
    </div>
  );
}

export function EmptyState({ icon, title, description, action, compact = false }: { icon: LucideIcon; title: string; description?: string; action?: ReactNode; compact?: boolean }) {
  return (
    <div className={cx("so-empty", compact && "is-compact")}>
      <span className="so-empty-icon"><Icon icon={icon} size={20} /></span>
      <h3 className="so-empty-title">{title}</h3>
      {description && <p className="so-empty-description">{description}</p>}
      {action && <div className="so-empty-action">{action}</div>}
    </div>
  );
}

export function Card({ as: Tag = "section", className, children, ...rest }: { as?: "section" | "div" | "article" | "li"; className?: string; children: ReactNode } & React.HTMLAttributes<HTMLElement>) {
  return <Tag className={cx("so-card", className)} {...(rest as object)}>{children}</Tag>;
}

let writeClipboard: (value: string) => Promise<void> = (value) => navigator.clipboard.writeText(value);
/** The host app may route copying through its shell (a sandboxed window has no clipboard permission). */
export function setClipboardWriter(writer: (value: string) => Promise<void>): void { writeClipboard = writer; }

/** A command or config the user copies. The copy button announces the result. */
export function CopyBlock({ label, value, copyLabel, copiedLabel, multiline = false }: { label: string; value: string; copyLabel: string; copiedLabel: string; multiline?: boolean }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);
  useEffect(() => () => { if (timer.current !== null) window.clearTimeout(timer.current); }, []);
  const copy = () => {
    void writeClipboard(value).then(() => {
      setCopied(true);
      if (timer.current !== null) window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(false), 1600);
    }).catch(() => {});
  };
  return (
    <div className="so-copy" data-multiline={multiline ? "true" : undefined}>
      <div className="so-copy-head">
        <span className="so-copy-label">{label}</span>
        <Button size="compact" variant="ghost" icon={copied ? Check : Copy} onClick={copy} disabled={!value} aria-label={`${copyLabel}: ${label}`}>{copied ? copiedLabel : copyLabel}</Button>
      </div>
      <pre className="so-copy-value" tabIndex={0} aria-label={label}>{value}</pre>
      <span className="so-visually-hidden" role="status">{copied ? copiedLabel : ""}</span>
    </div>
  );
}

