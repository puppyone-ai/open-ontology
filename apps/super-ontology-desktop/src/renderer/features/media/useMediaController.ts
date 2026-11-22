import { useCallback, useEffect, useMemo, useRef, useState } from "react";

export type PlayRange = { startMs: number; endMs: number };

export type MediaController = {
  /** Attach to the `<audio>` or `<video>` element. */
  attach: (element: HTMLMediaElement | null) => void;
  playing: boolean;
  /** Where the player is, in milliseconds; moves while it plays. */
  currentMs: number;
  durationMs: number;
  /** Who asked for what is playing (a speaker label or `file`), or `null`. */
  owner: string | null;
  /** The media element could not play the file. */
  failed: boolean;
  /** Plays one stretch and stops at its end. */
  playRange: (range: PlayRange, owner?: string) => void;
  /** Plays stretches one after the other (a speaker's clips). */
  playQueue: (ranges: readonly PlayRange[], owner: string) => void;
  /** Moves to a time; plays from there unless told not to. */
  seek: (ms: number, options?: { play?: boolean; owner?: string }) => void;
  toggle: () => void;
  pause: () => void;
};

/**
 * One player for the whole screen: the review rows ask it for a speaker's clips and the preview pane shows it. A stretch plays to its end
 * and stops (or moves on to the next of a queue). `currentMs` follows the element on every frame while it plays, so the transcript and the
 * time line stay in step without a timer of ours running when nothing plays.
 */
export function useMediaController(): MediaController {
  const element = useRef<HTMLMediaElement | null>(null);
  const range = useRef<{ queue: PlayRange[]; index: number } | null>(null);
  const frame = useRef(0);
  const [playing, setPlaying] = useState(false);
  const [currentMs, setCurrentMs] = useState(0);
  const [durationMs, setDurationMs] = useState(0);
  const [owner, setOwner] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  const sync = useCallback(() => { if (element.current) setCurrentMs(Math.round(element.current.currentTime * 1000)); }, []);

  const stop = useCallback(() => {
    range.current = null;
    element.current?.pause();
  }, []);

  const tick = useCallback(() => {
    const media = element.current;
    if (!media) return;
    const now = media.currentTime * 1000;
    setCurrentMs(Math.round(now));
    const active = range.current;
    if (active) {
      const { endMs } = active.queue[active.index];
      if (now >= endMs) {
        if (active.index + 1 < active.queue.length) {
          active.index += 1;
          media.currentTime = active.queue[active.index].startMs / 1000;
        } else { range.current = null; media.pause(); setOwner(null); return; }
      }
    }
    if (!media.paused) frame.current = window.requestAnimationFrame(tick);
  }, []);

  const attach = useCallback((next: HTMLMediaElement | null) => {
    element.current = next;
    if (!next) return;
    next.addEventListener("play", () => { setPlaying(true); setFailed(false); window.cancelAnimationFrame(frame.current); frame.current = window.requestAnimationFrame(tick); });
    next.addEventListener("pause", () => { setPlaying(false); window.cancelAnimationFrame(frame.current); sync(); });
    next.addEventListener("ended", () => { setPlaying(false); range.current = null; setOwner(null); sync(); });
    next.addEventListener("seeked", sync);
    next.addEventListener("timeupdate", sync);
    next.addEventListener("loadedmetadata", () => { setDurationMs(Number.isFinite(next.duration) ? Math.round(next.duration * 1000) : 0); });
    next.addEventListener("durationchange", () => { setDurationMs(Number.isFinite(next.duration) ? Math.round(next.duration * 1000) : 0); });
    next.addEventListener("error", () => { setFailed(true); setPlaying(false); });
  }, [sync, tick]);

  useEffect(() => () => window.cancelAnimationFrame(frame.current), []);

  const start = useCallback((queue: PlayRange[], who: string) => {
    const media = element.current;
    if (!media || queue.length === 0) return;
    range.current = { queue, index: 0 };
    setOwner(who);
    media.currentTime = queue[0].startMs / 1000;
    void media.play().catch(() => { setFailed(true); });
  }, []);

  return useMemo<MediaController>(() => ({
    attach, playing, currentMs, durationMs, owner, failed,
    playRange: (next, who = "file") => start([next], who),
    playQueue: (ranges, who) => start([...ranges], who),
    seek: (ms, options = {}) => {
      const media = element.current;
      if (!media) return;
      range.current = null;
      media.currentTime = Math.max(0, ms) / 1000;
      setCurrentMs(Math.max(0, ms));
      setOwner(options.owner ?? "file");
      if (options.play !== false) void media.play().catch(() => { setFailed(true); });
    },
    toggle: () => {
      const media = element.current;
      if (!media) return;
      if (media.paused) { range.current = null; setOwner("file"); void media.play().catch(() => { setFailed(true); }); } else stop();
    },
    pause: stop,
  }), [attach, playing, currentMs, durationMs, owner, failed, start, stop]);
}
