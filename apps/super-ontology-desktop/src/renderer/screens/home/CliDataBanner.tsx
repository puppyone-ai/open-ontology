import { useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Notice } from "@open-ontology/design-system";
import { invoke } from "../../model/bridgeInvoke";
import { shouldOfferCliData } from "../../model/cliDataOffer";
import { useLoad } from "../../hooks/useLoad";

const KEY = "so.cliData.offer.dismissed";
const read = () => { try { return window.localStorage.getItem(KEY) === "1"; } catch { return false; } };

/** A calm, one-time offer on Home for an install that runs on its own state while the CLI has data of its own. It reuses the Settings > Engine action. */
export function CliDataBanner() {
  const { t } = useLocalization();
  const [dismissed, setDismissed] = useState(read);
  const state = useLoad(() => invoke("engine-state-read", {}), []);
  if (!shouldOfferCliData(state.value, dismissed)) return null;
  return (
    <div style={{ marginBottom: "var(--so-space-5)" }} data-testid="cli-data-banner">
      <Notice tone="accent" title={t("app.home.banner.cli.title")} action={<span className="so-row">
        <Button size="compact" variant="primary" onClick={() => { void invoke("engine-data-use", { mode: "cli" }); }} data-testid="cli-data-use">{t("app.home.banner.cli.use")}</Button>
        <Button size="compact" variant="ghost" onClick={() => { try { window.localStorage.setItem(KEY, "1"); } catch { /* shown again next time */ } setDismissed(true); }} data-testid="cli-data-later">{t("app.home.banner.cli.later")}</Button>
      </span>}>{t("app.home.banner.cli.body")}</Notice>
    </div>
  );
}
