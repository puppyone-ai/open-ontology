import { useLocalization } from "@open-ontology/design-system/localization";

/** The document profiles the engine gives a file (docs-api FileSummary.profile); they decide what is extracted from it. */
export const FILE_PROFILES = ["business", "meeting", "table", "instruction", "code", "reference", "media", "other"] as const;
export type FileProfile = (typeof FILE_PROFILES)[number];

/** What the ontology does with a file (D94): only records put entities in it; the rest is attached, referenced, structural, media or archive only. */
export const FILE_CLASSES = ["record", "instruction", "reference", "code", "media", "other"] as const;
export type FileClass = (typeof FILE_CLASSES)[number];

/** Why the engine chose the profile (docs-api FileSummary.profile_reason). */
export const PROFILE_REASONS = ["frontmatter", "name", "dir", "ext", "format", "content", "default", "model", "user"] as const;
export type ProfileReason = (typeof PROFILE_REASONS)[number];

/** The profile of a file summary; a missing or unknown value (an older or newer engine) reads as `other`, never as an error. */
export function profileOf(file: { profile?: unknown }): FileProfile {
  return typeof file.profile === "string" && (FILE_PROFILES as readonly string[]).includes(file.profile) ? (file.profile as FileProfile) : "other";
}

/** The class a profile belongs to (the engine's rule, used when an engine does not send `class`). */
export function classOfProfile(profile: FileProfile): FileClass {
  switch (profile) {
    case "business": case "meeting": case "table": return "record";
    default: return profile;
  }
}

/** The class of a file summary: its own `class`, else derived from the profile. An unknown value reads as `other`, never as an error. */
export function classOf(file: { class?: unknown; profile?: unknown }): FileClass {
  if (typeof file.class === "string") return (FILE_CLASSES as readonly string[]).includes(file.class) ? (file.class as FileClass) : "other";
  return classOfProfile(profileOf(file));
}

/** Why the profile was chosen, or `null` when the engine did not say or the value is not known. */
export function reasonOf(file: { profile_reason?: unknown }): ProfileReason | null {
  return typeof file.profile_reason === "string" && (PROFILE_REASONS as readonly string[]).includes(file.profile_reason) ? (file.profile_reason as ProfileReason) : null;
}

/** The name of a file's profile in the language of the app. */
export function useProfileText(): (file: { profile?: unknown }) => string {
  const { t } = useLocalization();
  return (file) => t(`app.sources.profile.${profileOf(file)}`);
}

/** The words for a file's class (the chip), its one-line meaning, and the reason, in the language of the app. */
export function useClassText(): {
  name: (file: { class?: unknown; profile?: unknown }) => string;
  /** "Category: Code file, not in the ontology · Based on: file extension" (the reason part is left out when unknown). */
  line: (file: { class?: unknown; profile?: unknown; profile_reason?: unknown }) => string;
} {
  const { t } = useLocalization();
  return {
    name: (file) => t(`app.sources.class.${classOf(file)}`),
    line: (file) => {
      const reason = reasonOf(file);
      const head = t("app.sources.panel.classLine", { what: t(`app.sources.class.detail.${classOf(file)}`) });
      return reason ? `${head}${t("app.sources.panel.reasonPart", { why: t(`app.sources.reason.${reason}`) })}` : head;
    },
  };
}
