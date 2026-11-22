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

/** `S1` is `A`, `S2` is `B`, ... `S27` is `AA`: the letter a person sees for a file-local speaker label (the engine uses the same rule). */
export function speakerLetter(label: string): string {
  const match = /^S(\d+)$/u.exec(label);
  let n = match ? Number(match[1]) : 0;
  if (n < 1) return label;
  let out = "";
  while (n > 0) { n -= 1; out = String.fromCharCode(65 + (n % 26)) + out; n = Math.floor(n / 26); }
  return out;
}

/** `《周会》的说话人 A · 1a2b3c4d` (D98), `Speaker A · 1a2b3c4d` and the older `说话人 S1 · …`: the placeholder Person of one speaker, without the part that makes it unique per file. */
const PLACEHOLDER = /^(?:Speaker|说话人) (\S+) · [0-9a-f]{4,}$/u;
const MEETING_PLACEHOLDER = /^《(.+)》的说话人 (\S+) · [0-9a-f]{4,}$/u;
const MEETING_NAME = /^(《.+》) · [0-9a-f]{8}$/u;
const letterOf = (raw: string) => (/^S\d+$/u.test(raw) ? speakerLetter(raw) : /^\d+$/u.test(raw) ? speakerLetter(`S${raw}`) : raw);

/** The letter and meeting title of a placeholder Person's name (`《周会》的说话人 A · 1a2b3c4d` gives `A` and `周会`); `null` for a real name. */
export function placeholderInfo(name: string): { letter: string; title: string | null } | null {
  const trimmed = name.trim();
  const meeting = MEETING_PLACEHOLDER.exec(trimmed);
  if (meeting) return { letter: letterOf(meeting[2]), title: meeting[1] };
  const plain = PLACEHOLDER.exec(trimmed);
  return plain ? { letter: letterOf(plain[1]), title: null } : null;
}

/** The letter of a placeholder Person's name; `null` for a real name. */
export function placeholderLetter(name: string): string | null {
  return placeholderInfo(name)?.letter ?? null;
}

/** An entity name as a person reads it: a speaker placeholder shows in the person's language ("《周会》的说话人 A"), a meeting loses its uniqueness suffix, anything else is unchanged. */
export function displayName(name: string, speakerText: (letter: string, title: string | null) => string): string {
  const info = placeholderInfo(name);
  if (info) return speakerText(info.letter, info.title);
  const meeting = MEETING_NAME.exec(name.trim());
  return meeting ? meeting[1] : name;
}

export type Clip = { id: string; speaker: string; startMs: number; endMs: number; text: string; lowConfidence: boolean };

const MERGE_GAP_MS = 1500;
const CLIP_MIN_MS = 5000;
const CLIP_MAX_MS = 10_000;
const CLIP_APART_MS = 5000;

/**
 * Where to listen to one speaker: the longest clean stretches of what they say, each 5 to 10 seconds (2 to 3 of them). Neighbouring
 * utterances of the same speaker that follow each other within 1.5 s count as one stretch; a longer one is cut to its first 10 s and the
 * text is cut in the same proportion (the engine gives no word times here, so the cut is approximate). Clips of low-confidence
 * text come last, and two clips start at least 5 s apart. A short recording gives what it has, possibly one clip.
 */
export function representativeClips(utterances: readonly Utterance[], speaker: string, count = 3): Clip[] {
  const stretches: Clip[] = [];
  let open: Clip | undefined;
  for (const utterance of utterances) {
    if (utterance.speaker !== speaker) { if (open) stretches.push(open); open = undefined; continue; }
    const previous: Clip | undefined = open;
    if (previous && utterance.startMs - previous.endMs <= MERGE_GAP_MS) {
      open = { ...previous, endMs: Math.max(previous.endMs, utterance.endMs), text: `${previous.text} ${utterance.text}`.trim(), lowConfidence: previous.lowConfidence || utterance.lowConfidence };
    } else {
      if (previous) stretches.push(previous);
      open = { id: utterance.id, speaker, startMs: utterance.startMs, endMs: utterance.endMs, text: utterance.text, lowConfidence: utterance.lowConfidence };
    }
  }
  if (open) stretches.push(open);
  const cut = (clip: Clip): Clip => {
    const length = clip.endMs - clip.startMs;
    if (length <= CLIP_MAX_MS) return clip;
    const keep = Math.max(1, Math.round(clip.text.length * (CLIP_MAX_MS / length)));
    return { ...clip, endMs: clip.startMs + CLIP_MAX_MS, text: clip.text.slice(0, keep).trim() };
  };
  const score = (clip: Clip) => Math.min(clip.endMs - clip.startMs, CLIP_MAX_MS) + (clip.endMs - clip.startMs >= CLIP_MIN_MS ? 100_000 : 0) - (clip.lowConfidence ? 1_000_000 : 0);
  const picked: Clip[] = [];
  for (const clip of [...stretches].filter((entry) => entry.endMs - entry.startMs >= 800).sort((a, b) => score(b) - score(a) || a.startMs - b.startMs)) {
    if (picked.length === count) break;
    if (picked.some((other) => Math.abs(other.startMs - clip.startMs) < CLIP_APART_MS)) continue;
    picked.push(cut(clip));
  }
  return picked.sort((a, b) => a.startMs - b.startMs);
}

/** A `#t=` mark of an evidence anchor (`t=00:12:31-00:12:48@S1`, or the whole `[src:…#t=…]`) as the second it starts at; `null` when the anchor is not a time. */
export function anchorSeconds(anchor: string): number | null {
  const match = /(?:^|#)t=([0-9:]+)/u.exec(anchor);
  return match ? parseClock(match[1]) : null;
}
