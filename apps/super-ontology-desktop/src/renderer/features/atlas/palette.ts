import type { AtlasTone } from "./model";

/**
 * Concrete colours of the tones (25 section 4): the ink-blue accent and the status colours, used only as low-saturation tints on the
 * warm white canvas. These mirror the design tokens (`--so-accent`, `--so-success`, `--so-warning`, `--so-danger`, stone text); a canvas
 * cannot read CSS variables per cell cheaply, so they are resolved once here.
 */
export type Rgb = readonly [number, number, number];
export const TONE_RGB: Record<AtlasTone, Rgb> = {
  ink: [47, 74, 208],
  steel: [64, 104, 160],
  dusk: [96, 92, 170],
  mist: [76, 132, 150],
  sand: [143, 122, 96],
  stone: [108, 104, 100],
};
export const CANVAS = "#fafaf9";
export const TEXT = "#44403c";
export const TEXT_WEAK = "#78716c";
export const rgba = ([r, g, b]: Rgb, alpha: number): string => `rgba(${r}, ${g}, ${b}, ${alpha})`;
