import tokens from "../../design/tokens.json" with { type: "json" };

type Mode = "light" | "dark";

export function contrastRatio(foreground: string, background: string): number {
  const lighter = Math.max(luminance(foreground), luminance(background));
  const darker = Math.min(luminance(foreground), luminance(background));
  return (lighter + 0.05) / (darker + 0.05);
}

function luminance(hex: string): number {
  const value = hex.replace("#", "");
  const channels = [0, 2, 4].map((offset) => {
    const channel = Number.parseInt(value.slice(offset, offset + 2), 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * channels[0]! + 0.7152 * channels[1]! + 0.0722 * channels[2]!;
}

export function token(name: keyof typeof tokens.primitive, mode: Mode): string {
  return tokens.primitive[name][mode];
}

export const contrastPairs: Array<{ name: string; minimum: number; foreground: keyof typeof tokens.primitive; background: keyof typeof tokens.primitive }> = [
  { name: "text/canvas", minimum: 4.5, foreground: "neutral-text", background: "neutral-canvas" },
  { name: "text/surface", minimum: 4.5, foreground: "neutral-text", background: "neutral-surface" },
  { name: "text/subtle", minimum: 4.5, foreground: "neutral-text", background: "neutral-subtle" },
  { name: "muted/canvas", minimum: 4.5, foreground: "neutral-muted", background: "neutral-canvas" },
  { name: "muted/surface", minimum: 4.5, foreground: "neutral-muted", background: "neutral-surface" },
  { name: "muted/subtle", minimum: 4.5, foreground: "neutral-muted", background: "neutral-subtle" },
  { name: "accent/canvas", minimum: 4.5, foreground: "blue-action", background: "neutral-canvas" },
  { name: "accent/surface", minimum: 4.5, foreground: "blue-action", background: "neutral-surface" },
  { name: "on-accent/accent", minimum: 4.5, foreground: "on-action", background: "blue-action" },
  { name: "success/surface", minimum: 4.5, foreground: "success-text", background: "neutral-surface" },
  { name: "warning/surface", minimum: 4.5, foreground: "warning-text", background: "neutral-surface" },
  { name: "danger/surface", minimum: 4.5, foreground: "danger-text", background: "neutral-surface" },
  { name: "success/bg", minimum: 4.5, foreground: "success-text", background: "success-bg" },
  { name: "warning/bg", minimum: 4.5, foreground: "warning-text", background: "warning-bg" },
  { name: "danger/bg", minimum: 4.5, foreground: "danger-text", background: "danger-bg" },
  { name: "text/accent-bg", minimum: 4.5, foreground: "neutral-text", background: "accent-bg" },
  { name: "input/surface", minimum: 3, foreground: "input-border", background: "neutral-surface" },
  { name: "input/canvas", minimum: 3, foreground: "input-border", background: "neutral-canvas" },
  { name: "focus/canvas", minimum: 3, foreground: "blue-action", background: "neutral-canvas" },
];
