// Placeholder app icon, drawn in code (no image tools, no third-party artwork): a white rounded square with an axis-symmetric
// node-link mark in the ink-blue accent. Writes build/icon.svg and build/icon.icns (macOS `iconutil`). The owner replaces both with final artwork.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import zlib from "node:zlib";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const buildDir = path.join(root, "build");

// Geometry on a 1024 canvas.
const BODY = { x: 100, y: 100, size: 824, radius: 186 };
const NODES = [[512, 512, 46], [512, 262, 34], [512, 762, 34], [300, 330, 34], [724, 330, 34], [300, 694, 34], [724, 694, 34]];
const LINKS = [[1, 0], [2, 0], [3, 0], [4, 0], [5, 0], [6, 0]];
const STROKE = 22;
// White tile, ink-blue mark (accent #2F4AD0 of the app palette).
const BG_TOP = [255, 255, 255]; const BG_BOTTOM = [244, 244, 242]; const FG = [47, 74, 208];

function distanceToSegment(px, py, ax, ay, bx, by) {
  const dx = bx - ax; const dy = by - ay;
  const t = Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

function insideBody(x, y, inset = 0) {
  const { x: bx, y: by, size, radius } = { x: BODY.x + inset, y: BODY.y + inset, size: BODY.size - 2 * inset, radius: BODY.radius - inset };
  const cx = Math.max(bx + radius, Math.min(bx + size - radius, x));
  const cy = Math.max(by + radius, Math.min(by + size - radius, y));
  return Math.hypot(x - cx, y - cy) <= radius;
}

/** Colour and alpha of one canvas point. */
function sample(x, y) {
  if (!insideBody(x, y)) return [0, 0, 0, 0];
  if (!insideBody(x, y, 5)) return [214, 211, 209, 1];
  for (const [a, b] of LINKS) {
    if (distanceToSegment(x, y, NODES[a][0], NODES[a][1], NODES[b][0], NODES[b][1]) <= STROKE / 2) return [...FG, 1];
  }
  for (const [nx, ny, r] of NODES) if (Math.hypot(x - nx, y - ny) <= r) return [...FG, 1];
  const mix = (y - BODY.y) / BODY.size;
  return [0, 1, 2].map((i) => BG_TOP[i] + (BG_BOTTOM[i] - BG_TOP[i]) * mix).concat(1);
}

function render(size) {
  const scale = 1024 / size; const grid = 3;
  const pixels = Buffer.alloc(size * size * 4);
  for (let py = 0; py < size; py += 1) {
    for (let px = 0; px < size; px += 1) {
      let r = 0; let g = 0; let b = 0; let a = 0;
      for (let sy = 0; sy < grid; sy += 1) {
        for (let sx = 0; sx < grid; sx += 1) {
          const [cr, cg, cb, ca] = sample((px + (sx + 0.5) / grid) * scale, (py + (sy + 0.5) / grid) * scale);
          r += cr * ca; g += cg * ca; b += cb * ca; a += ca;
        }
      }
      const offset = (py * size + px) * 4;
      if (a > 0) { pixels[offset] = Math.round(r / a); pixels[offset + 1] = Math.round(g / a); pixels[offset + 2] = Math.round(b / a); }
      pixels[offset + 3] = Math.round((a / (grid * grid)) * 255);
    }
  }
  return pixels;
}

const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
const crc32 = (buffer) => { let c = 0xffffffff; for (const byte of buffer) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
function chunk(type, data) {
  const length = Buffer.alloc(4); length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}
function png(size) {
  const pixels = render(size);
  const rows = Buffer.alloc((size * 4 + 1) * size);
  for (let y = 0; y < size; y += 1) { rows[y * (size * 4 + 1)] = 0; pixels.copy(rows, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4); }
  const header = Buffer.alloc(13); header.writeUInt32BE(size, 0); header.writeUInt32BE(size, 4); header[8] = 8; header[9] = 6;
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", header), chunk("IDAT", zlib.deflateSync(rows, { level: 9 })), chunk("IEND", Buffer.alloc(0))]);
}

function svg() {
  const lines = LINKS.map(([a, b]) => `<path d="M${NODES[a][0]} ${NODES[a][1]}L${NODES[b][0]} ${NODES[b][1]}"/>`).join("");
  const dots = NODES.map(([x, y, r]) => `<circle cx="${x}" cy="${y}" r="${r}"/>`).join("");
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="rgb(${BG_TOP})"/><stop offset="1" stop-color="rgb(${BG_BOTTOM})"/></linearGradient></defs><rect x="${BODY.x}" y="${BODY.y}" width="${BODY.size}" height="${BODY.size}" rx="${BODY.radius}" fill="url(#g)" stroke="rgb(214,211,209)" stroke-width="5"/><g fill="none" stroke="rgb(${FG})" stroke-width="${STROKE}" stroke-linecap="round">${lines}</g><g fill="rgb(${FG})">${dots}</g></svg>\n`;
}

fs.mkdirSync(buildDir, { recursive: true });
fs.writeFileSync(path.join(buildDir, "icon.svg"), svg());
const iconset = path.join(buildDir, "icon.iconset");
fs.rmSync(iconset, { recursive: true, force: true });
fs.mkdirSync(iconset);
for (const [name, size] of [["icon_16x16", 16], ["icon_16x16@2x", 32], ["icon_32x32", 32], ["icon_32x32@2x", 64], ["icon_128x128", 128], ["icon_128x128@2x", 256],
  ["icon_256x256", 256], ["icon_256x256@2x", 512], ["icon_512x512", 512], ["icon_512x512@2x", 1024]]) {
  fs.writeFileSync(path.join(iconset, `${name}.png`), png(size));
}
if (process.platform === "darwin") {
  execFileSync("iconutil", ["-c", "icns", iconset, "-o", path.join(buildDir, "icon.icns")]);
  fs.rmSync(iconset, { recursive: true, force: true });
  console.log("icon.icns written");
} else {
  console.log("iconset written (iconutil is macOS only)");
}
