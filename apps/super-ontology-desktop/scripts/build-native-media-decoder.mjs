// Builds the macOS host media decoder (native/super-drive-media-decoder, AVFoundation) into build/native/super-drive-media-decoder/.
// The packaged app carries it as Resources/super-drive-media-decoder/ (electron-builder.yml, extraResources): the engine's own FFmpeg has no
// H.264/HEVC/AAC decoder by design (D65), so an .m4a or .mp4 is decoded here and the engine reads the result from the exchange directory.
// Off macOS this does nothing (the app has no helper there and says so).
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
if (process.platform !== "darwin") { console.log("host media decoder: skipped (not macOS)"); process.exit(0); }
const source = path.join(root, "native", "super-drive-media-decoder", "main.swift");
const output = path.join(root, "build", "native", "super-drive-media-decoder");
fs.mkdirSync(output, { recursive: true });
const binary = path.join(output, "super-drive-media-decoder");
execFileSync("swiftc", ["-O", source, "-o", binary], { stdio: "inherit" });
fs.chmodSync(binary, 0o755);
console.log(`host media decoder built: ${binary} (${execFileSync(binary, ["--version"], { encoding: "utf8" }).trim()})`);
