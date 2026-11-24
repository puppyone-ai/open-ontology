import { useLocalization } from "@open-ontology/design-system/localization";
import { Card, Section } from "@open-ontology/design-system";
import { Cloud, Cpu, HardDrive, ShieldCheck } from "lucide-react";
import { useDataFlow } from "../../model/useDataFlow";

const ICON = { filesLocal: HardDrive, filesBucket: Cloud, results: ShieldCheck, model: Cpu } as const;

/** Where your data goes, computed from what is configured right now; it cannot promise something that isn't true. */
export function Privacy() {
  const { t } = useLocalization();
  const flow = useDataFlow();
  return (
    <div className="so-stack" data-testid="privacy">
      <Section title={t("app.privacy.title")} description={t("app.privacy.body")}>
        <Card className="so-list">
          {flow.destinations.map((destination, index) => {
            const Glyph = ICON[destination.kind];
            return (
              <div key={`${destination.kind}:${index}`} className="so-list-row" data-destination={destination.kind}>
                <span className="so-list-lead"><Glyph size={16} strokeWidth={1.5} aria-hidden="true" /></span>
                <span className="so-list-text"><span>{t(destination.messageId, destination.values)}</span></span>
              </div>
            );
          })}
        </Card>
      </Section>
    </div>
  );
}
