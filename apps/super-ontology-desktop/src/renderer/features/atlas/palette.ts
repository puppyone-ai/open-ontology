import type { AtlasTone } from "./model";

/**
 * Concrete colours of the atlas (25 section 4). A canvas cannot read CSS variables per cell cheaply, so they are resolved once here.
 * Tones are the ink-blue accent's quiet relatives; the accent itself is kept for selection and focus. Fills are solid, low saturation.
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
export const ACCENT: Rgb = [47, 74, 208];
export const CANVAS = "#fafaf9";
export const TEXT = "#1c1917";
export const TEXT_SECOND = "#57534e";
export const TEXT_WEAK = "#a8a29e";
export const rgba = ([r, g, b]: Rgb, alpha: number): string => `rgba(${r}, ${g}, ${b}, ${alpha})`;

const mix = (a: Rgb, b: Rgb, t: number): Rgb => [Math.round(a[0] + (b[0] - a[0]) * t), Math.round(a[1] + (b[1] - a[1]) * t), Math.round(a[2] + (b[2] - a[2]) * t)];
const hex = ([r, g, b]: Rgb): string => `#${[r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
const GREY: Rgb = [128, 124, 120];
/** The solid fill of a tone: its colour greyed and lightened, so a field of them stays calm. */
export const TONE_FILL: Record<AtlasTone, string> = Object.fromEntries((Object.keys(TONE_RGB) as AtlasTone[]).map((tone) => [tone, hex(mix(mix(TONE_RGB[tone], GREY, 0.26), [250, 249, 249], 0.1))])) as Record<AtlasTone, string>;
/** Outlines and rings of a tone: a darker step of the same colour. */
export const TONE_INK: Record<AtlasTone, Rgb> = Object.fromEntries((Object.keys(TONE_RGB) as AtlasTone[]).map((tone) => [tone, mix(TONE_RGB[tone], GREY, 0.18)])) as Record<AtlasTone, Rgb>;
