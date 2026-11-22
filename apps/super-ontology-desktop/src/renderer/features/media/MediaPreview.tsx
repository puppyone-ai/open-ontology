import { useEffect, useMemo, useRef } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Badge, Button, IconButton, Notice, Skeleton } from "@open-ontology/design-system";
import { Pause, Play } from "lucide-react";
import { formatTimecode, utteranceAt } from "../../model/details/mediaModel";
import type { MediaController } from "./useMediaController";
import type { MediaClassInfo, MediaSource } from "./useMediaSource";

/** The class a recording was given, as a badge: a conversation, not one, or not sure yet; the reason is its tooltip. */
export function MediaClassBadge({ info }: { info: MediaClassInfo | null }) {
  const { t } = useLocalization();
  if (!info) return null;
  const tone = info.class === "conversation" ? "accent" : "neutral";
  const why = t(`app.media.kind.${info.kind}`);
  return <span title={why} data-testid="media-class" data-class={info.class}><Badge tone={tone}>{t(`app.media.class.${info.class}`)}</Badge></span>;
}

/** Plays the file and shows its transcript beside the time line: the row that is being said is marked, a click on any row plays from there. */
export function MediaPreview({ source, controller, startMs, autoplay = false, compact = false, speakerText }: {
  source: MediaSource;
  controller: MediaController;
  /** Move here once the recording is ready (an evidence chip's `#t=`). */
  startMs?: number;
  autoplay?: boolean;
  compact?: boolean;
  /** How a label such as `S1` reads to a person (the localised placeholder). */
  speakerText: (label: string) => string;
}) {
  const { t } = useLocalization();
  const list = useRef<HTMLOListElement>(null);
  const ready = source.state === "ready" ? source : null;
  const { attach, seek, currentMs, durationMs, playing, failed, toggle } = controller;
  const active = useMemo(() => (ready ? utteranceAt(ready.utterances, currentMs / 1000) : null), [ready, currentMs]);
  const activeId = active && currentMs <= active.endMs + 1500 ? active.id : null;
  const started = useRef<string | null>(null);

  useEffect(() => {
    if (!ready || startMs === undefined || started.current === `${ready.fileId}:${startMs}`) return;
    started.current = `${ready.fileId}:${startMs}`;
    seek(startMs, { play: autoplay, owner: "file" });
  }, [ready, startMs, autoplay, seek]);

  useEffect(() => {
    if (!activeId || !playing) return;
    list.current?.querySelector<HTMLElement>(`[data-utterance="${activeId}"]`)?.scrollIntoView?.({ block: "nearest" });
  }, [activeId, playing]);

  if (source.state === "loading") return <div className="so-media" aria-busy="true"><Skeleton height={52} radius={8} /><Skeleton height={13} width="70%" /><Skeleton height={13} width="55%" /></div>;
  if (source.state === "unavailable") return <Notice tone="warning" title={t("app.media.unavailable.title")} testId="media-unavailable">{t(source.reason === "s3" ? "app.media.unavailable.s3" : "app.media.unavailable.other")}</Notice>;
  const total = durationMs || (source.utterances.at(-1)?.endMs ?? 0);
  return (
    <div className="so-media" data-testid="media-pane" data-kind={source.kind} data-playing={playing ? "true" : "false"} data-compact={compact ? "true" : undefined}>
      {source.kind === "video"
        ? <video ref={attach} className="so-media-video" src={source.url} preload="metadata" playsInline aria-label={source.name} data-testid="media-element" />
        : <audio ref={attach} src={source.url} preload="metadata" data-testid="media-element" />}
      <div className="so-media-bar">
        <IconButton icon={playing ? Pause : Play} label={t(playing ? "app.media.pause" : "app.media.play")} size="compact" onClick={toggle} data-testid="media-play" />
        <span className="so-mono so-media-time" data-testid="media-time" aria-hidden="true">{formatTimecode(currentMs)}</span>
        <input type="range" className="so-media-seek" min={0} max={Math.max(1, total)} step={100} value={Math.min(currentMs, Math.max(1, total))} aria-label={t("app.media.position")}
          aria-valuetext={`${formatTimecode(currentMs)} / ${formatTimecode(total)}`} onChange={(event) => seek(Number(event.target.value), { play: playing })} data-testid="media-seek" />
        <span className="so-mono so-media-time" aria-hidden="true">{formatTimecode(total)}</span>
      </div>
      {failed && <Notice tone="danger" title={t("app.media.failed")} action={<Button size="compact" onClick={source.reload}>{t("app.media.retry")}</Button>} testId="media-failed">{t("app.media.failedBody")}</Notice>}
      {source.utterances.length === 0
        ? <p className="so-weak" data-testid="media-no-transcript">{t("app.media.transcript.empty")}</p>
        : (
          <ol ref={list} className="so-transcript" aria-label={t("app.media.transcript")} data-testid="media-transcript">
            {source.utterances.map((utterance) => (
              <li key={utterance.id}>
                <button type="button" className="so-transcript-row" data-utterance={utterance.id} data-testid="media-utterance" data-start={utterance.startMs} aria-current={utterance.id === activeId ? "true" : undefined}
                  onClick={() => seek(utterance.startMs, { owner: "file" })}>
                  <span className="so-mono so-transcript-time">{formatTimecode(utterance.startMs)}</span>
                  <span className="so-transcript-body">
                    {utterance.speaker && <span className="so-transcript-speaker">{speakerText(utterance.speaker)}</span>}
                    <span className="so-transcript-text">{utterance.text}</span>
                  </span>
                </button>
              </li>
            ))}
          </ol>
        )}
    </div>
  );
}
