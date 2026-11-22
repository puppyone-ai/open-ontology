import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Dialog } from "@open-ontology/design-system";
import { speakerLetter } from "../../model/details/mediaModel";
import { MediaClassBadge, MediaPreview } from "./MediaPreview";
import { useMediaController } from "./useMediaController";
import { useMediaSource } from "./useMediaSource";

/** An evidence chip of a recording (`#t=`): the recording opens at that second and plays, with its transcript, without leaving the page. */
export function MediaPreviewDialog({ fileId, title, startSeconds, onClose, onOpenFile }: { fileId: string; title: string; startSeconds: number; onClose: () => void; onOpenFile: () => void }) {
  const { t } = useLocalization();
  const controller = useMediaController();
  const source = useMediaSource(fileId);
  return (
    <Dialog title={title} size="md" onClose={onClose} closeLabel={t("app.media.close")} testId="media-dialog"
      footer={<Button onClick={onOpenFile}>{t("app.media.openFile")}</Button>}>
      <div className="so-stack">
        <MediaClassBadge info={source.state === "ready" ? source.info : null} />
        <MediaPreview source={source} controller={controller} startMs={startSeconds * 1000} autoplay speakerText={(label) => t("app.speaker.name", { letter: speakerLetter(label) })} />
      </div>
    </Dialog>
  );
}
