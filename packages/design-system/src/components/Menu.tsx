import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import type { LucideIcon } from "lucide-react";
import { Icon } from "../icons/Icon";
import { cx } from "./cx";

type Placement = "bottom-start" | "bottom-end" | "right-start";

/** A floating layer anchored to an element: closes on Escape, an outside press, or blur of the page; keeps itself inside the window. */
export function Popover({ anchor, open, onClose, placement = "bottom-start", children, className, role = "dialog", ariaLabel }: {
  anchor: RefObject<HTMLElement | null>; open: boolean; onClose: () => void; placement?: Placement; children: ReactNode; className?: string; role?: string; ariaLabel?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    if (!open) { setPos(null); return; }
    const layer = ref.current;
    const target = anchor.current;
    if (!layer || !target) return;
    const a = target.getBoundingClientRect();
    const w = layer.offsetWidth;
    const h = layer.offsetHeight;
    let left = placement === "bottom-end" ? a.right - w : placement === "right-start" ? a.right + 4 : a.left;
    let top = placement === "right-start" ? a.top : a.bottom + 4;
    if (top + h + 8 > window.innerHeight) top = Math.max(8, (placement === "right-start" ? window.innerHeight - h - 8 : a.top - h - 4));
    left = Math.min(Math.max(8, left), window.innerWidth - w - 8);
    setPos({ left: Math.round(left), top: Math.round(top) });
  }, [open, anchor, placement, children]);

  useEffect(() => {
    if (!open) return undefined;
    const onDown = (event: PointerEvent) => {
      const node = event.target as Node;
      if (ref.current?.contains(node) || anchor.current?.contains(node)) return;
      onClose();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.stopPropagation(); onClose(); anchor.current?.focus({ preventScroll: true }); }
    };
    document.addEventListener("pointerdown", onDown, true);
    document.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", onClose);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      document.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onClose);
    };
  }, [open, anchor, onClose]);

  if (!open) return null;
  return createPortal(
    <div ref={ref} className={cx("so-popover", className)} role={role} aria-label={ariaLabel} style={{ left: pos?.left ?? 0, top: pos?.top ?? 0, visibility: pos ? "visible" : "hidden" }}>
      {children}
    </div>,
    document.body,
  );
}

/** A list of actions in a Popover. Arrow keys, Home, End and type-ahead move; Enter or Space picks; focus starts on the first enabled item. */
export function Menu({ anchor, open, onClose, children, ariaLabel, placement }: { anchor: RefObject<HTMLElement | null>; open: boolean; onClose: () => void; children: ReactNode; ariaLabel: string; placement?: Placement }) {
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const first = listRef.current?.querySelector<HTMLElement>("[role='menuitem']:not([aria-disabled='true'])");
    first?.focus({ preventScroll: true });
  }, [open]);
  const onKeyDown = (event: React.KeyboardEvent) => {
    const items = Array.from(listRef.current?.querySelectorAll<HTMLElement>("[role='menuitem']:not([aria-disabled='true'])") ?? []);
    if (items.length === 0) return;
    const index = items.indexOf(document.activeElement as HTMLElement);
    let next = -1;
    if (event.key === "ArrowDown") next = (index + 1) % items.length;
    else if (event.key === "ArrowUp") next = (index - 1 + items.length) % items.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = items.length - 1;
    else if (event.key === "Tab") { onClose(); return; }
    else if (event.key.length === 1 && !event.metaKey && !event.ctrlKey) {
      const lower = event.key.toLowerCase();
      next = items.findIndex((item, i) => i > index && (item.textContent ?? "").trim().toLowerCase().startsWith(lower));
      if (next < 0) next = items.findIndex((item) => (item.textContent ?? "").trim().toLowerCase().startsWith(lower));
    }
    if (next >= 0) { event.preventDefault(); items[next]?.focus({ preventScroll: true }); }
  };
  return (
    <Popover anchor={anchor} open={open} onClose={onClose} role="presentation" placement={placement}>
      <div ref={listRef} className="so-menu" role="menu" aria-label={ariaLabel} onKeyDown={onKeyDown}>{children}</div>
    </Popover>
  );
}

export function MenuItem({ icon, label, shortcut, tone, disabled, onSelect, closeMenu }: { icon?: LucideIcon; label: string; shortcut?: string; tone?: "danger"; disabled?: boolean; onSelect: () => void; closeMenu?: () => void }) {
  return (
    <button
      type="button"
      role="menuitem"
      className="so-menu-item"
      data-tone={tone}
      aria-disabled={disabled || undefined}
      tabIndex={-1}
      onClick={() => { if (disabled) return; closeMenu?.(); onSelect(); }}
    >
      <span className="so-menu-item-icon">{icon && <Icon icon={icon} />}</span>
      <span className="so-menu-item-label">{label}</span>
      {shortcut && <kbd className="so-menu-item-kbd">{shortcut}</kbd>}
    </button>
  );
}

export function MenuSeparator() {
  return <div className="so-menu-separator" role="separator" />;
}
