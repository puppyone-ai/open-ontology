import { useId, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { CopyBlock, Notice } from "@open-ontology/design-system";
import { ChevronRight, Terminal } from "lucide-react";
import type { CliError } from "../../../shared/super-ontology/bridge";
import { useErrorText } from "../../hooks/useText";
import { describeCliError } from "../../model/cliApi";
import { hasSuperDriveMessage } from "../../model/superDriveMessageKeys";

/** Test ids derived from a `testId` prop (`<id>-toggle`, `<id>-value`, `<id>-copy`); spelled out here so TEST-IDS.md and its conformance test can find them. */
export const DERIVED_TEST_IDS = ["database-import-cli-toggle", "database-import-cli-value", "query-cli-toggle", "query-cli-value", "query-describe-cli-toggle"] as const;

/** "Equivalent command" (D88): every action says which `oo` command does the same thing, one click to copy. */
export function CliCommand({ command, testId = "cli-command" }: { command: string; testId?: string }) {
  const { t } = useLocalization();
  const [open, setOpen] = useState(false);
  const id = useId();
  if (!command) return null;
  return (
    <div className="so-cli" data-testid={testId} data-open={open ? "true" : undefined}>
      <button type="button" className="so-cli-toggle" aria-expanded={open} aria-controls={id} onClick={() => setOpen((v) => !v)} data-testid={`${testId}-toggle`}>
        <ChevronRight className="so-cli-chevron" size={14} strokeWidth={1.5} aria-hidden="true" />
        <Terminal size={14} strokeWidth={1.5} aria-hidden="true" />
        <span>{t("app.cli.equivalent")}</span>
      </button>
      {open && <div id={id}><CopyBlock label={t("app.cli.command")} value={command} copyLabel={t("app.common.copy")} copiedLabel={t("app.common.copied")} multiline valueTestId={`${testId}-value`} copyTestId={`${testId}-copy`} /></div>}
    </div>
  );
}

/** What a failed command says: the code, what `oo` itself printed (its own text, the cause), then a sentence for the code in the language of the app. */
export function CliErrorText({ error, testId = "cli-error" }: { error: CliError; testId?: string }) {
  const { t } = useLocalization();
  const bridgeText = useErrorText();
  const view = describeCliError(error, (code) => hasSuperDriveMessage(`super-drive.errors.${code}`));
  const sentence = view.sentence === "next" ? t(`app.cli.next.${error.code}`)
    : view.sentence === "bridge" ? bridgeText({ code: error.code, retryable: false, details: {} })
      : t("app.cli.error.generic");
  return (
    <>
      <span className="so-cli-error-code so-mono">{error.code}</span>
      {view.raw && <p className="so-cli-error-text" data-testid={`${testId}-raw`}>{view.raw}</p>}
      <p className="so-cli-error-next" data-testid={view.sentence === "next" ? `${testId}-next` : undefined}>{sentence}</p>
    </>
  );
}

/** A failed command: the sentence for its code, what the CLI said, and the command to run by hand. */
export function CliErrorNotice({ error, cli, title, testId = "cli-error" }: { error: CliError; cli?: string; title: string; testId?: string }) {
  return (
    <Notice tone="danger" title={title} testId={testId}>
      <CliErrorText error={error} testId={testId} />
      {cli && <CliCommand command={cli} testId={`${testId}-cli`} />}
    </Notice>
  );
}
