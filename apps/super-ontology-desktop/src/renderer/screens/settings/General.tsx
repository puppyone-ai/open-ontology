import { useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Card, Section, Select } from "@open-ontology/design-system";
import { isSelectableAppLanguagePreference, resolveSystemLocale } from "@open-ontology/design-system/localization/core";
import { useApp } from "../../hooks/useApp";
import { Button } from "@open-ontology/design-system";

/** Language of the interface: live (no restart), saved, "same as the system" names the system's language, each language is written in itself. */
function LanguageSetting() {
  const { t, preference, status, setLanguagePreference, localeDescriptors, systemLanguages } = useLocalization();
  const [failed, setFailed] = useState(false);
  const systemLocale = resolveSystemLocale(systemLanguages);
  const systemLanguage = Object.values(localeDescriptors).find((d) => d.locale === systemLocale)?.label ?? systemLocale;
  const change = async (value: string) => {
    if (!isSelectableAppLanguagePreference(value)) return;
    setFailed(false);
    try { await setLanguagePreference(value); } catch { setFailed(true); }
  };
  return (
    <Card className="so-form">
      <Select label={t("app.settings.language")} value={preference} disabled={status === "changing"} onChange={(event) => { void change(event.target.value); }} data-testid="settings-language" data-value={preference}>
        <option value="system">{t("app.settings.language.system", { language: systemLanguage })}</option>
        {Object.values(localeDescriptors).map((d) => <option key={d.locale} value={d.locale} lang={d.locale} dir={d.direction}>{d.label}</option>)}
      </Select>
      {status === "changing" && <small className="so-weak" role="status">{t("app.settings.language.changing")}</small>}
      {(failed || status === "error") && <small className="so-language-error" role="alert">{t("app.settings.language.changeFailed")}</small>}
    </Card>
  );
}

export function General() {
  const { t } = useLocalization();
  const app = useApp();
  return (
    <div className="so-stack">
      <Section title={t("app.settings.language.title")} description={t("app.settings.language.body")}>
        <LanguageSetting />
      </Section>
      <Section title={t("app.settings.welcome.title")} description={t("app.settings.welcome.body")}>
        <div><Button onClick={app.openFirstRun}>{t("app.settings.welcome.action")}</Button></div>
      </Section>
    </div>
  );
}
