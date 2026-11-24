import { useEffect, useId, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { IconButton } from "./Button";
import { cx } from "./cx";

const FOCUSABLE = ["button:not([disabled])", "[href]", "input:not([disabled])", "select:not([disabled])", "textarea:not([disabled])", "[tabindex]:not([tabindex='-1'])"].join(",");
const openDialogs: HTMLElement[] = [];

export type DialogProps = {
  title: string;
  description?: string;
  onClose?: () => void;
  size?: "sm" | "md" | "lg";
  /** `action-required` dialogs ignore Escape and the backdrop. */
  dismissal?: "dismissible" | "action-required";
  footer?: ReactNode;
  children?: ReactNode;
  className?: string;
  testId?: string;
  closeLabel?: string;
};

/** Modal dialog: focus moves in, is trapped, and returns to the opener; Escape and the backdrop close it; only the top dialog reacts. */
export function Dialog({ title, description, onClose, size = "md", dismissal = "dismissible", footer, children, className, testId, closeLabel = "Close" }: DialogProps) {
  const titleId = useId();
  const descriptionId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  const dismissRef = useRef(dismissal);
  dismissRef.current = dismissal;
  const downOnBackdrop = useRef(false);

  useEffect(() => {
    const root = rootRef.current;
    const surface = surfaceRef.current;
    if (!root || !surface) return undefined;
    openDialogs.push(root);
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    (surface.querySelector<HTMLElement>("[data-initial-focus], [autofocus]") ?? surface.querySelector<HTMLElement>("input:not([disabled]), textarea:not([disabled])") ?? surface).focus({ preventScroll: true });
    const onKey = (event: KeyboardEvent) => {
      if (openDialogs[openDialogs.length - 1] !== root) return;
      if (event.key === "Escape") {
        if (event.isComposing || dismissRef.current === "action-required" || !closeRef.current) return;
        event.preventDefault();
        event.stopPropagation();
        closeRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const items = Array.from(surface.querySelectorAll<HTMLElement>(FOCUSABLE));
      if (items.length === 0) { event.preventDefault(); surface.focus(); return; }
      const first = items[0]!;
      const last = items[items.length - 1]!;
      const active = document.activeElement;
      if (!surface.contains(active) || active === surface) { event.preventDefault(); (event.shiftKey ? last : first).focus(); }
      else if (event.shiftKey && active === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && active === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      const index = openDialogs.indexOf(root);
      if (index >= 0) openDialogs.splice(index, 1);
      if (opener?.isConnected) opener.focus({ preventScroll: true });
    };
  }, []);

  return createPortal(
    <div
      ref={rootRef}
      className="so-dialog-backdrop"
      role="presentation"
      onPointerDown={(event) => { downOnBackdrop.current = event.target === event.currentTarget; }}
      onClick={(event) => {
        if (dismissal === "dismissible" && downOnBackdrop.current && event.target === event.currentTarget) onClose?.();
        downOnBackdrop.current = false;
      }}
    >
      <div ref={surfaceRef} className={cx("so-dialog", className)} data-size={size} role="dialog" aria-modal="true" aria-labelledby={titleId} aria-describedby={description ? descriptionId : undefined} tabIndex={-1} data-testid={testId}>
        <header className="so-dialog-header">
          <div className="so-dialog-heading">
            <h2 id={titleId} className="so-dialog-title">{title}</h2>
            {description && <p id={descriptionId} className="so-dialog-description">{description}</p>}
          </div>
          {onClose && dismissal === "dismissible" && <IconButton icon={X} label={closeLabel} size="compact" onClick={onClose} />}
        </header>
        {children != null && <div className="so-dialog-body">{children}</div>}
        {footer != null && <footer className="so-dialog-footer">{footer}</footer>}
      </div>
    </div>,
    document.body,
  );
}
