import { useLocalization } from "@open-ontology/design-system/localization";
import { Card, Section, Select } from "@open-ontology/design-system";
import type { AppLanguagePreference } from "@open-ontology/design-system/localization/core";
import { useApp } from "../../hooks/useApp";
import { Button } from "@open-ontology/design-system";

export function General() {
  const { t, preference, setLanguagePreference, localeDescriptors } = useLocalization();
  const app = useApp();
  return (
    <div className="so-stack">
      <Section title={t("app.settings.language.title")} description={t("app.settings.language.body")}>
        <Card className="so-form">
          <Select label={t("app.settings.language")} value={preference} onChange={(event) => { void setLanguagePreference(event.target.value as AppLanguagePreference); }}>
            <option value="system">{t("app.settings.language.system")}</option>
            {Object.values(localeDescriptors).map((d) => <option key={d.locale} value={d.locale}>{d.label}</option>)}
          </Select>
        </Card>
      </Section>
      <Section title={t("app.settings.welcome.title")} description={t("app.settings.welcome.body")}>
        <div><Button onClick={app.openFirstRun}>{t("app.settings.welcome.action")}</Button></div>
      </Section>
    </div>
  );
}
