import { useEffect, useState } from "react";

/** `prefers-reduced-motion: reduce`, live: the atlas switches to its static layout the moment the person asks for less motion. */
export function useReducedMotion(): boolean {
  const query = typeof window !== "undefined" && typeof window.matchMedia === "function" ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
  const [reduced, setReduced] = useState(query?.matches ?? false);
  useEffect(() => {
    if (!query) return undefined;
    const listener = (event: MediaQueryListEvent) => setReduced(event.matches);
    query.addEventListener("change", listener);
    return () => query.removeEventListener("change", listener);
  }, [query]);
  return reduced;
}
