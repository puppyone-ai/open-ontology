import type { EngineBlock } from "../../../shared/super-ontology/bridge";

export type MediaKind = "audio" | "video";

const AUDIO = new Set(["audio", "mp3", "m4a", "wav", "flac", "ogg", "opus", "aac", "wma", "aiff"]);
const VIDEO = new Set(["video", "mp4", "mov", "mkv", "webm", "avi", "m4v", "mpg", "mpeg"]);

/** Whether a file is a recording, from the format the engine reports or, failing that, the extension. */
export function mediaKindOf(format: string | null | undefined, path: string): MediaKind | null {
  const known = (format ?? "").toLowerCase();
  if (AUDIO.has(known)) return "audio";
  if (VIDEO.has(known)) return "video";
  const extension = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
  return AUDIO.has(extension) ? "audio" : VIDEO.has(extension) ? "video" : null;
}

/** `h:mm:ss` (or `m:ss` under an hour), the way a player shows a position. */
export function formatTimecode(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const two = (value: number) => String(value).padStart(2, "0");
  return hours > 0 ? `${hours}:${two(minutes)}:${two(seconds)}` : `${minutes}:${two(seconds)}`;
}

const TIME = /^(\d+):(\d{1,2}):(\d{1,2})$/u;
/** `00:12:31` (the form of the provenance fragments) to seconds; `null` for anything else. */
export function parseClock(text: string): number | null {
  const match = TIME.exec(text.trim());
  return match ? Number(match[1]) * 3600 + Number(match[2]) * 60 + Number(match[3]) : null;
}

/** The start a provenance label points at: `#t=00:12:31-00:12:48`, `#t=00:12:31@S1` and `#f=00:03:05` all give seconds. */
export function sourceSeconds(label: string): number | null {
  const match = /#[tf]=([0-9:]+)/u.exec(label);
  return match ? parseClock(match[1]) : null;
}

export type Utterance = { id: string; startMs: number; endMs: number; speaker: string | null; text: string; lowConfidence: boolean; src: string | null };

type MediaAnchor = { kind: "media"; t_start_ms: number; t_end_ms: number; speaker?: string };
const isMediaAnchor = (anchor: unknown): anchor is MediaAnchor => typeof anchor === "object" && anchor !== null && (anchor as { kind?: unknown }).kind === "media";

/** The timestamped speech of a recording, in time order. */
export function utterancesOf(blocks: readonly EngineBlock[]): Utterance[] {
  return blocks
    .filter((block) => block.kind === "utterance" && isMediaAnchor(block.anchor))
    .map((block) => {
      const anchor = block.anchor as unknown as MediaAnchor;
      return { id: block.id, startMs: anchor.t_start_ms, endMs: anchor.t_end_ms, speaker: anchor.speaker ?? null, text: block.text, lowConfidence: block.flags?.includes("low_text_confidence") === true, src: block.src ?? null };
    })
    .sort((left, right) => left.startMs - right.startMs);
}

export type KeyFrame = { id: string; atMs: number; text: string };

/** Frame text of a video (`frame_text` blocks) as the strip shows it. */
export function keyFramesOf(blocks: readonly EngineBlock[]): KeyFrame[] {
  return blocks
    .filter((block) => block.kind === "frame_text" && (block.anchor as { kind?: string } | undefined)?.kind === "frame")
    .map((block) => ({ id: block.id, atMs: (block.anchor as unknown as { t_ms: number }).t_ms, text: block.text }))
    .sort((left, right) => left.atMs - right.atMs);
}

export type SpeakerSummary = { label: string; durationMs: number; utterances: number; representative: Utterance | null };

/** Who spoke how long, with the longest utterance as the sample to play. Labels are only valid inside this file (D74). */
export function speakersOf(utterances: readonly Utterance[]): SpeakerSummary[] {
  const byLabel = new Map<string, SpeakerSummary>();
  for (const utterance of utterances) {
    if (!utterance.speaker) continue;
    const entry = byLabel.get(utterance.speaker) ?? { label: utterance.speaker, durationMs: 0, utterances: 0, representative: null };
    entry.durationMs += Math.max(0, utterance.endMs - utterance.startMs);
    entry.utterances += 1;
    if (!entry.representative || utterance.endMs - utterance.startMs > entry.representative.endMs - entry.representative.startMs) entry.representative = utterance;
    byLabel.set(utterance.speaker, entry);
  }
  return [...byLabel.values()].sort((left, right) => left.label.localeCompare(right.label, undefined, { numeric: true }));
}

/** The utterance that covers a time (the one a `#t=` mark points into), or the nearest one starting before it. */
export function utteranceAt(utterances: readonly Utterance[], seconds: number): Utterance | null {
  const ms = seconds * 1000;
  let best: Utterance | null = null;
  for (const utterance of utterances) {
    if (utterance.startMs <= ms) best = utterance; else break;
  }
  return best;
}
