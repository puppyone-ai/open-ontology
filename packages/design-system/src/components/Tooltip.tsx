import { cloneElement, useCallback, useEffect, useId, useRef, useState, type ReactElement } from "react";
import { createPortal } from "react-dom";

const SHOW_DELAY_MS = 450;
const GAP = 6;
const MARGIN = 8;

type TriggerProps = {
  onPointerEnter?: (event: React.PointerEvent) => void;
  onPointerLeave?: (event: React.PointerEvent) => void;
  onFocus?: (event: React.FocusEvent) => void;
  onBlur?: (event: React.FocusEvent) => void;
  onPointerDown?: (event: React.PointerEvent) => void;
  "aria-describedby"?: string;
};

/**
 * Names a control or shows text that was cut off. It shows after a short hover or on keyboard focus, hides on blur, press
 * and Escape, never takes focus, and stays inside the window.
 */
export function Tooltip({ label, shortcut, children, disabled = false }: { label: string; shortcut?: string; children: ReactElement<TriggerProps>; disabled?: boolean }) {
  const id = useId();
  const triggerRef = useRef<HTMLElement | null>(null);
  const timer = useRef<number | null>(null);
  const [open, setOpen] = useState(false);
  const [box, setBox] = useState<{ x: number; y: number; flip: boolean } | null>(null);

  const clear = () => { if (timer.current !== null) { window.clearTimeout(timer.current); timer.current = null; } };
  const hide = useCallback(() => { clear(); setOpen(false); setBox(null); }, []);
  const schedule = (delay: number) => { clear(); timer.current = window.setTimeout(() => setOpen(true), delay); };
  useEffect(() => clear, []);
  useEffect(() => {
    if (!open) return undefined;
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") hide(); };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open, hide]);

  const props = children.props;
  const child = cloneElement(children, {
    ...(disabled || !label ? {} : { "aria-describedby": open ? id : props["aria-describedby"] }),
    onPointerEnter: (event: React.PointerEvent) => { props.onPointerEnter?.(event); triggerRef.current = event.currentTarget as HTMLElement; if (!disabled && event.pointerType !== "touch") schedule(SHOW_DELAY_MS); },
    onPointerLeave: (event: React.PointerEvent) => { props.onPointerLeave?.(event); hide(); },
    onPointerDown: (event: React.PointerEvent) => { props.onPointerDown?.(event); hide(); },
    onFocus: (event: React.FocusEvent) => { props.onFocus?.(event); triggerRef.current = event.currentTarget as HTMLElement; if (!disabled && event.currentTarget.matches(":focus-visible")) schedule(0); },
    onBlur: (event: React.FocusEvent) => { props.onBlur?.(event); hide(); },
  } as TriggerProps);

  return (
    <>
      {child}
      {open && !disabled && label && triggerRef.current && createPortal(
        <TooltipBubble id={id} label={label} shortcut={shortcut} target={triggerRef.current} box={box} onBox={setBox} />,
        document.body,
      )}
    </>
  );
}

function TooltipBubble({ id, label, shortcut, target, box, onBox }: { id: string; label: string; shortcut?: string; target: HTMLElement; box: { x: number; y: number; flip: boolean } | null; onBox: (box: { x: number; y: number; flip: boolean }) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const bubble = ref.current;
    if (!bubble) return;
    const rect = target.getBoundingClientRect();
    const width = bubble.offsetWidth;
    const height = bubble.offsetHeight;
    const flip = rect.bottom + GAP + height + MARGIN > window.innerHeight;
    const x = Math.min(Math.max(MARGIN, rect.left + rect.width / 2 - width / 2), window.innerWidth - width - MARGIN);
    const y = flip ? rect.top - GAP - height : rect.bottom + GAP;
    onBox({ x: Math.round(x), y: Math.round(y), flip });
  }, [target, label, shortcut, onBox]);
  return (
    <div ref={ref} id={id} role="tooltip" className="so-tooltip" data-flip={box?.flip ? "true" : undefined} style={{ left: box?.x ?? 0, top: box?.y ?? 0, visibility: box ? "visible" : "hidden" }}>
      <span>{label}</span>
      {shortcut && <kbd className="so-tooltip-kbd">{shortcut}</kbd>}
    </div>
  );
}
