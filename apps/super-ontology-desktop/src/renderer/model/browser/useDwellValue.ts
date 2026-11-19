import { useEffect, useRef, useState } from "react";

/**
 * Keeps showing `value` for at least `minimumMs` before switching to the next
 * one, so a status that flips back and forth in a blink does not flicker. The
 * latest value always wins in the end (intermediate ones are skipped). Nothing
 * is shown yet when `shown` is null or undefined, so the first value appears at once.
 */
export function useDwellValue<T>(value: T, minimumMs: number): T {
  const [shown, setShown] = useState(value);
  const shownSince = useRef(Date.now());
  const latest = useRef(value);
  latest.current = value;
  useEffect(() => {
    if (Object.is(shown, value)) return undefined;
    const elapsed = Date.now() - shownSince.current;
    const delay = shown === null || shown === undefined ? 0 : Math.max(0, minimumMs - elapsed);
    const apply = () => { shownSince.current = Date.now(); setShown(latest.current); };
    if (delay === 0) { apply(); return undefined; }
    const handle = setTimeout(apply, delay);
    return () => clearTimeout(handle);
  }, [value, shown, minimumMs]);
  return shown;
}
