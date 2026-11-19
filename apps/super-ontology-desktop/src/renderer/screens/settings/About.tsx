import { useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, DefinitionList, Section } from "@open-ontology/design-system";
import { AppMark } from "../../shell/AppMark";

declare const __APP_VERSION__: string;

type NoticeKind = "notice" | "third-party" | "license";
const NOTICE_KINDS: readonly NoticeKind[] = ["notice", "third-party", "license"];

export function About() {
  const { t } = useLocalization();
  const [shown, setShown] = useState<NoticeKind | null>(null);
  const [text, setText] = useState("");
  const hosted = window.superOntology?.shell?.readNotice;
  const toggle = async (kind: NoticeKind) => {
    if (shown === kind) { setShown(null); return; }
    setText(await window.superOntology!.shell!.readNotice!(kind).catch(() => ""));
    setShown(kind);
  };
  return (
    <div className="so-stack" data-testid="about">
      <Card className="so-form">
        <div className="so-row"><span style={{ color: "var(--so-accent)" }}><AppMark size={28} /></span><div className="so-stack" style={{ gap: 2 }}><span className="so-list-title" style={{ fontSize: "var(--so-text-md)" }}>Super Ontology</span><span className="so-weak" data-testid="about-version">{t("app.about.version", { version: __APP_VERSION__ })}</span></div></div>
        <p className="so-muted">{t("app.about.body")}</p>
      </Card>
      <Section title={t("app.about.license.title")}>
        <Card className="so-form">
          <DefinitionList items={[
            { label: t("app.about.license"), value: "Apache License 2.0" },
            { label: t("app.about.notices"), value: t("app.about.notices.body") },
          ]} />
          {hosted && (
            <div className="so-stack">
              <div className="so-row" data-wrap>{NOTICE_KINDS.map((kind) => <Button key={kind} size="compact" aria-pressed={shown === kind} onClick={() => { void toggle(kind); }} data-testid={`about-${kind}`}>{t(`app.about.show.${kind}`)}</Button>)}</div>
              {shown && <pre className="so-legal" tabIndex={0} aria-label={t(`app.about.show.${shown}`)}>{text}</pre>}
            </div>
          )}
        </Card>
      </Section>
    </div>
  );
}
