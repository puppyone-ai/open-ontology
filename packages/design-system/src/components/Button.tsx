import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import { Icon } from "../icons/Icon";
import { Spinner } from "./Spinner";
import { Tooltip } from "./Tooltip";
import { cx } from "./cx";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: "default" | "compact";
  icon?: LucideIcon;
  /** Replaces the icon (or, without one, overlays the label) with a spinner of the same size; the button keeps its width. */
  loading?: boolean;
  children?: ReactNode;
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "default", icon, loading = false, disabled, className, children, type = "button", ...rest },
  ref,
) {
  const overlay = loading && !icon;
  return (
    <button
      ref={ref}
      type={type}
      className={cx("so-button", className)}
      data-variant={variant}
      data-size={size}
      data-loading={loading ? "true" : undefined}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...rest}
    >
      {icon && (loading ? <Spinner size={16} className="so-button-glyph" /> : <Icon icon={icon} className="so-button-glyph" />)}
      {children != null && <span className="so-button-label" style={overlay ? { visibility: "hidden" } : undefined}>{children}</span>}
      {overlay && <Spinner size={16} className="so-button-overlay" />}
    </button>
  );
});

export type IconButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children" | "aria-label"> & {
  icon: LucideIcon;
  /** Required: it is the accessible name and the tooltip. */
  label: string;
  variant?: "ghost" | "secondary";
  size?: "default" | "compact";
  loading?: boolean;
  shortcut?: string;
};

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { icon, label, variant = "ghost", size = "default", loading = false, shortcut, disabled, className, type = "button", ...rest },
  ref,
) {
  return (
    <Tooltip label={label} shortcut={shortcut}>
      <button
        ref={ref}
        type={type}
        className={cx("so-icon-button", className)}
        data-variant={variant}
        data-size={size}
        aria-label={label}
        disabled={disabled || loading}
        aria-busy={loading || undefined}
        {...rest}
      >
        {loading ? <Spinner size={16} /> : <Icon icon={icon} />}
      </button>
    </Tooltip>
  );
});
