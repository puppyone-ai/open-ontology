import type { LucideIcon } from "lucide-react";

export type IconSize = 16 | 20;

/**
 * The one icon wrapper: a single line set, 16px in lists and buttons and 20px in navigation, 1.5px stroke that does not
 * scale with the box, `currentColor`, and a fixed box so an icon never shifts the text beside it.
 */
export function Icon({ icon: Glyph, size = 16, className }: { icon: LucideIcon; size?: IconSize; className?: string }) {
  return <Glyph className={className ? `so-icon ${className}` : "so-icon"} size={size} strokeWidth={1.5} absoluteStrokeWidth aria-hidden="true" focusable="false" />;
}
