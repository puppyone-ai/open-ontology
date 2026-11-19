import type { AtlasTone } from "./model";

/**
 * Concrete colours of the tones (25 section 4): the ink-blue accent and the status colours, used only as low-saturation tints on the
 * warm white canvas. These mirror the design tokens (`--so-accent`, `--so-success`, `--so-warning`, `--so-danger`, stone text); a canvas
 * cannot read CSS variables per cell cheaply, so they are resolved once here.
 */
export type Rgb = readonly [number, number, number];
export const TONE_RGB: Record<AtlasTone, Rgb> = {
  ink: [47, 74, 208],
  moss: [21, 128, 61],
  sand: [180, 83, 9],
  clay: [185, 28, 28],
  stone: [87, 83, 78],
};
export const CANVAS = "#fafaf9";
export const TEXT = "#1c1917";
export const TEXT_WEAK = "#78716c";
export const rgba = ([r, g, b]: Rgb, alpha: number): string => `rgba(${r}, ${g}, ${b}, ${alpha})`;
