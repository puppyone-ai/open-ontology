import { useLocalization } from "@open-ontology/design-system/localization";
import { PageHeader, Tabs } from "@open-ontology/design-system";
import { useApp, type SettingsTab } from "../hooks/useApp";
import { General } from "./settings/General";
import { EngineSettings } from "./settings/EngineSettings";
import { ModelSettings } from "./settings/ModelSettings";
import { Privacy } from "./settings/Privacy";
import { About } from "./settings/About";

export function SettingsScreen() {
  const { t } = useLocalization();
  const app = useApp();
  return (
    <div className="so-page" data-testid="screen-settings">
      <PageHeader title={t("app.settings.title")} />
      <div style={{ marginBottom: "var(--so-space-5)" }}>
        <Tabs<SettingsTab> label={t("app.settings.title")} value={app.settingsTab} onChange={app.setSettingsTab}
          tabs={[{ value: "general", label: t("app.settings.tab.general"), testId: "settings-section-general" }, { value: "engine", label: t("app.settings.tab.engine"), testId: "settings-section-engine" }, { value: "model", label: t("app.settings.tab.model"), testId: "settings-section-model" }, { value: "privacy", label: t("app.settings.tab.privacy"), testId: "settings-section-privacy" }, { value: "about", label: t("app.settings.tab.about"), testId: "settings-section-about" }]} />
      </div>
      {app.settingsTab === "general" && <General />}
      {app.settingsTab === "engine" && <EngineSettings />}
      {app.settingsTab === "model" && <ModelSettings />}
      {app.settingsTab === "privacy" && <Privacy />}
      {app.settingsTab === "about" && <About />}
    </div>
  );
}
