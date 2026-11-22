import { useLocalization } from "@open-ontology/design-system/localization";
import { displayName } from "../model/details/mediaModel";

/** An entity name as a person reads it: the placeholder of an unnamed speaker ("Speaker A · 1a2b3c4d") shows as "Speaker A" in the active language. */
export function useDisplayName(): (name: string) => string {
  const { t } = useLocalization();
  return (name) => displayName(name, (letter) => t("app.speaker.name", { letter }));
}
