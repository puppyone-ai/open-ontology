import { useLocalization } from "@open-ontology/design-system/localization";

/** The document profiles the engine gives a file (docs-api FileSummary.profile); they decide what is extracted from it. */
export const FILE_PROFILES = ["business", "meeting", "table", "instruction", "code", "paper", "media", "other"] as const;
export type FileProfile = (typeof FILE_PROFILES)[number];

/** The profile of a file summary; a missing or unknown value (an older or newer engine) reads as `other`, never as an error. */
export function profileOf(file: { profile?: unknown }): FileProfile {
  return typeof file.profile === "string" && (FILE_PROFILES as readonly string[]).includes(file.profile) ? (file.profile as FileProfile) : "other";
}

/** The name of a file's profile in the language of the app. */
export function useProfileText(): (file: { profile?: unknown }) => string {
  const { t } = useLocalization();
  return (file) => t(`app.sources.profile.${profileOf(file)}`);
}
