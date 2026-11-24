import { cx } from "./cx";

/** Same box as the 16px icon it replaces, so swapping an icon for a spinner never moves the label. */
export function Spinner({ size = 16, className, label }: { size?: 12 | 16 | 20; className?: string; label?: string }) {
  return (
    <svg
      className={cx("so-spinner", className)}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      role={label ? "status" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      focusable="false"
    >
      <circle className="so-spinner-track" cx="8" cy="8" r="6" fill="none" strokeWidth="1.5" />
      <circle className="so-spinner-arc" cx="8" cy="8" r="6" fill="none" strokeWidth="1.5" strokeLinecap="round" pathLength="100" strokeDasharray="28 72" />
    </svg>
  );
}

/** A skeleton block with the exact size of the content it stands in for. */
export function Skeleton({ width, height = 12, radius = 4, className }: { width?: number | string; height?: number | string; radius?: number; className?: string }) {
  return <span className={cx("so-skeleton", className)} style={{ width: width ?? "100%", height, borderRadius: radius }} aria-hidden="true" />;
}

export function ProgressBar({ value, label, tone = "accent" }: { value: number | null; label: string; tone?: "accent" | "success" | "warning" | "danger" }) {
  const pct = value === null ? null : Math.max(0, Math.min(1, value));
  return (
    <div className="so-progress" data-tone={tone} data-indeterminate={pct === null ? "true" : undefined} role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct === null ? undefined : Math.round(pct * 100)}>
      <span className="so-progress-fill" style={pct === null ? undefined : { width: `${pct * 100}%` }} />
    </div>
  );
}
