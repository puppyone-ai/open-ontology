import { useId, type ButtonHTMLAttributes } from "react";

type ButtonVariant = "default" | "secondary" | "outline" | "ghost" | "destructive";
type ButtonSize = "sm" | "default";

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  readOnly?: boolean;
  disabledReason?: string;
};

const variantClass: Record<ButtonVariant, string> = {
  default: "",
  secondary: "oo-button-secondary",
  outline: "oo-button-outline",
  ghost: "oo-button-ghost",
  destructive: "oo-button-destructive",
};

export function Button({
  variant = "default",
  size = "default",
  loading = false,
  readOnly = false,
  disabledReason,
  disabled,
  children,
  className,
  id,
  ...props
}: ButtonProps) {
  const generated = useId();
  const reasonId = `${id ?? generated}-reason`;
  const inactive = Boolean(disabled || loading || readOnly);
  const classes = ["oo-button", variantClass[variant], size === "sm" ? "oo-button-sm" : "", className]
    .filter(Boolean)
    .join(" ");
  return (
    <>
      <button
        {...props}
        id={id}
        className={classes}
        disabled={inactive}
        aria-busy={loading || undefined}
        aria-describedby={inactive && disabledReason ? reasonId : undefined}
      >
        {children}
      </button>
      {inactive && disabledReason ? (
        <p id={reasonId} className="oo-help">
          {disabledReason}
        </p>
      ) : null}
    </>
  );
}
