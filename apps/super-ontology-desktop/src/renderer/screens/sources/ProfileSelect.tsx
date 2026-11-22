import { useLocalization } from "@open-ontology/design-system/localization";
import { Select } from "@open-ontology/design-system";
import { classOfProfile, FILE_PROFILES, type FileProfile } from "../../model/profile";

export const AUTO_PROFILE = "auto";

/** The eight profiles, each named with the class it belongs to, plus "back to automatic" (profile null). */
export function ProfileSelect({ label, value, disabled, testId, onChange }: { label: string; value: FileProfile | typeof AUTO_PROFILE | ""; disabled?: boolean; testId: string; onChange: (profile: FileProfile | null) => void }) {
  const { t } = useLocalization();
  return (
    <Select label={label} value={value} disabled={disabled} data-testid={testId}
      onChange={(event) => { const next = event.target.value; if (next === AUTO_PROFILE) onChange(null); else if ((FILE_PROFILES as readonly string[]).includes(next)) onChange(next as FileProfile); }}>
      {value === "" && <option value="" disabled>{t("app.sources.override.choose")}</option>}
      <option value={AUTO_PROFILE}>{t("app.sources.override.auto")}</option>
      {FILE_PROFILES.map((profile) => (
        <option key={profile} value={profile}>{t("app.sources.override.option", { profile: t(`app.sources.profile.${profile}`), class: t(`app.sources.class.${classOfProfile(profile)}`) })}</option>
      ))}
    </Select>
  );
}
