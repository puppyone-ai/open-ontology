import { useLocalization } from "@open-ontology/design-system/localization";
import { Card, DefinitionList, Section } from "@open-ontology/design-system";
import { AppMark } from "../../shell/AppMark";

declare const __APP_VERSION__: string;

export function About() {
  const { t } = useLocalization();
  return (
    <div className="so-stack" data-testid="about">
      <Card className="so-form">
        <div className="so-row"><span style={{ color: "var(--so-accent)" }}><AppMark size={28} /></span><div className="so-stack" style={{ gap: 2 }}><span className="so-list-title" style={{ fontSize: "var(--so-text-md)" }}>Super Ontology</span><span className="so-weak">{t("app.about.version", { version: __APP_VERSION__ })}</span></div></div>
        <p className="so-muted">{t("app.about.body")}</p>
      </Card>
      <Section title={t("app.about.license.title")}>
        <Card className="so-form">
          <DefinitionList items={[
            { label: t("app.about.license"), value: "Apache License 2.0" },
            { label: t("app.about.notices"), value: t("app.about.notices.body") },
          ]} />
        </Card>
      </Section>
    </div>
  );
}
