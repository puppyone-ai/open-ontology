import { useId, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, IconButton, LinkButton, TextField } from "@open-ontology/design-system";
import { Check, ExternalLink, Mic, Pause, Play, X } from "lucide-react";
import type { ReviewItem } from "../../../shared/super-ontology/docs-api.generated";
import { subjectView } from "../../model/review/reviewModel";
import { formatTimecode, representativeClips, speakerLetter } from "../../model/details/mediaModel";
import { usePersonNames } from "../../features/media/usePersonNames";
import type { MediaController } from "../../features/media/useMediaController";
import type { MediaSource } from "../../features/media/useMediaSource";

export type SpeakerDecide = (item: ReviewItem, decision: "accept" | "reject", edit?: { name: string }) => Promise<void>;

function SpeakerRow({ item, busy, onDecide, source, controller, active, names, onListen }: {
  item: ReviewItem; busy: string | null; onDecide: SpeakerDecide; source: MediaSource; controller: MediaController; active: boolean; names: string[]; onListen: (speaker: string) => void;
}) {
  const { t } = useLocalization();
  const listId = useId();
  const view = subjectView(item);
  const [typed, setTyped] = useState("");
  if (view.kind !== "speaker") return null;
  const label = t("app.speaker.name", { letter: view.letter || speakerLetter(view.speaker) });
  const candidates = [...new Map(view.candidates.map((candidate) => [candidate.name, candidate])).values()].slice(0, 3);
  const clips = source.state === "ready" && active ? representativeClips(source.utterances, view.speaker) : [];
  const playing = controller.playing && controller.owner === view.speaker;
  const disabled = busy !== null && busy !== item.id;
  const name = typed.trim();
  const listen = () => {
    if (playing) { controller.pause(); return; }
    if (clips.length > 0) controller.playQueue(clips, view.speaker); else onListen(view.speaker);
  };
  return (
    <div className="so-speaker-row" data-testid="speaker-row" data-speaker={view.speaker} data-playing={playing ? "true" : undefined}>
      <div className="so-speaker-head">
        <IconButton icon={playing ? Pause : Play} variant="secondary" size="compact" label={t(playing ? "app.review.speaker.pause" : "app.review.speaker.play", { speaker: label })} onClick={listen} data-testid="speaker-play" />
        <span className="so-speaker-label"><SpeakerMic />{label}</span>
      </div>
      {active && source.state === "ready" && clips.length === 0 && <p className="so-weak so-speaker-note" data-testid="speaker-no-clips">{t("app.review.speaker.noClips")}</p>}
      {clips.length > 0 && (
        <ul className="so-clip-list" aria-label={t("app.review.speaker.clips", { speaker: label })}>
          {clips.map((clip) => {
            const now = controller.playing && controller.owner === view.speaker && controller.currentMs >= clip.startMs && controller.currentMs < clip.endMs;
            return (
              <li key={clip.id} className="so-clip" data-testid="speaker-clip" data-start={clip.startMs} data-now={now ? "true" : undefined}>
                <IconButton icon={now ? Pause : Play} size="compact" label={t("app.review.speaker.playClip", { time: formatTimecode(clip.startMs) })}
                  onClick={() => { if (now) controller.pause(); else controller.playRange({ startMs: clip.startMs, endMs: clip.endMs }, view.speaker); }} data-testid="speaker-clip-play" />
                <span className="so-clip-body">
                  <span className="so-clip-text" data-testid="speaker-clip-text">{clip.text}</span>
                  <span className="so-clip-meta"><span className="so-mono">{formatTimecode(clip.startMs)}–{formatTimecode(clip.endMs)}</span>
                    <LinkButton icon={ExternalLink} onClick={() => controller.seek(clip.startMs, { owner: view.speaker })} data-testid="speaker-open-at">{t("app.review.speaker.openAt", { time: formatTimecode(clip.startMs) })}</LinkButton>
                  </span>
                </span>
              </li>
            );
          })}
        </ul>
      )}
      {candidates.length > 0 && (
        <div className="so-speaker-candidates">
          {candidates.map((candidate) => (
            <span key={candidate.name} className="so-speaker-candidate">
              <Button size="compact" variant="primary" icon={Check} loading={busy === item.id} disabled={disabled} data-testid="speaker-confirm-candidate" title={candidate.quote || undefined}
                onClick={() => { void onDecide(item, "accept", { name: candidate.name }); }}>{t("app.review.speaker.use", { name: candidate.name })}</Button>
              <span className="so-weak so-speaker-source">{t(candidate.source === "self_intro" ? "app.review.speaker.fromIntro" : candidate.source === "attendees" ? "app.review.speaker.fromAttendees" : "app.review.speaker.fromRecording")}</span>
            </span>
          ))}
        </div>
      )}
      <div className="so-speaker-name-row">
        <TextField labelHidden label={t("app.review.speaker.nameLabel", { speaker: label })} placeholder={t("app.review.speaker.namePlaceholder")} value={typed} maxLength={120} spellCheck={false} autoComplete="off"
          list={listId} data-testid="speaker-name" onChange={(event) => setTyped(event.target.value)}
          onKeyDown={(event) => { if (event.key === "Enter" && name !== "" && !disabled && !event.nativeEvent.isComposing) void onDecide(item, "accept", { name }); }} />
        <datalist id={listId}>{names.map((known) => <option key={known} value={known} />)}</datalist>
        <Button size="compact" icon={Check} loading={busy === item.id && name !== ""} disabled={disabled || name === ""} data-testid="speaker-confirm-typed" onClick={() => { void onDecide(item, "accept", { name }); }}>{t("app.review.speaker.save")}</Button>
        <Button size="compact" variant="ghost" icon={X} disabled={disabled} data-testid="speaker-none" onClick={() => { void onDecide(item, "reject"); }}>{t("app.review.speaker.none")}</Button>
      </div>
    </div>
  );
}

function SpeakerMic() { return <Mic size={14} strokeWidth={1.5} aria-hidden="true" className="so-speaker-mic" />; }

/**
 * Who is speaking in one recording (D74 as revised): every speaker is listed as "Speaker A", "Speaker B" with a play button for a few clear
 * stretches of their voice, so a name can be put to the voice. Nothing waits for it: the recording is already in the ontology under those
 * placeholders; a name given here moves what each said to the named person, and "Not anyone known" leaves it as it is.
 */
export function SpeakerReviewGroup({ items, fileName, driveName, busy, onDecide, source, controller, active, onActivate }: {
  items: ReviewItem[]; fileName: string; driveName: string; busy: string | null; onDecide: SpeakerDecide; source: MediaSource; controller: MediaController; active: boolean; onActivate: (speaker?: string) => void;
}) {
  const { t } = useLocalization();
  const names = usePersonNames(true);
  return (
    <section className="so-speaker-group" data-testid="speaker-group" data-active={active ? "true" : undefined} aria-label={t("app.review.speaker.groupLabel", { name: fileName })}>
      <header className="so-speaker-group-head">
        <button type="button" className="so-speaker-group-title" onClick={() => onActivate()} aria-pressed={active} data-testid="speaker-group-open">
          <span className="so-list-title so-truncate">{t("app.review.speaker.title", { name: fileName })}</span>
          <span className="so-list-sub so-truncate">{t("app.review.speaker.sub")} · {driveName}</span>
        </button>
      </header>
      {items.map((item) => <SpeakerRow key={item.id} item={item} busy={busy} onDecide={onDecide} source={source} controller={controller} active={active} names={names} onListen={(speaker) => onActivate(speaker)} />)}
    </section>
  );
}
