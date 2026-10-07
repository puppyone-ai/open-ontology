import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { contrastPairs, contrastRatio, token } from "../src/styles/contrast.ts";

const out = process.env.FE01_EVIDENCE;
if (!out) throw new Error("FE01_EVIDENCE is required");

const required = contrastPairs.flatMap((pair) =>
  (["light", "dark"] as const).map((mode) => {
    const foregroundHex = token(pair.foreground, mode);
    const backgroundHex = token(pair.background, mode);
    const ratio = contrastRatio(foregroundHex, backgroundHex);
    return {
      name: pair.name,
      mode,
      minimum: pair.minimum,
      foreground: pair.foreground,
      background: pair.background,
      foregroundHex,
      backgroundHex,
      ratio: Number(ratio.toFixed(3)),
      pass: ratio >= pair.minimum,
    };
  }),
);

const decorative = (["light", "dark"] as const).flatMap((mode) =>
  (["neutral-canvas", "neutral-surface"] as const).map((background) => {
    const foregroundHex = token("neutral-border", mode);
    const backgroundHex = token(background, mode);
    const ratio = contrastRatio(foregroundHex, backgroundHex);
    return {
      name: `decorative-border/${background}`,
      mode,
      role: "decorative",
      required: false,
      foregroundHex,
      backgroundHex,
      ratio: Number(ratio.toFixed(3)),
    };
  }),
);

await mkdir(out, { recursive: true });
await writeFile(join(out, "contrast-tokens.json"), `${JSON.stringify({ required, decorative }, null, 2)}\n`);
if (required.some((row) => !row.pass)) process.exitCode = 1;
