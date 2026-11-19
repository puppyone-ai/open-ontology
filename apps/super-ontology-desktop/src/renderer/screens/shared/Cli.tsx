import { useId, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { CopyBlock, Notice } from "@open-ontology/design-system";
import { ChevronRight, Terminal } from "lucide-react";
import type { CliError } from "../../../shared/super-ontology/bridge";
import { hasNextStep } from "../../model/cliApi";

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

/** A failed command: what the CLI said, what to do next (when we know), and the command to run by hand. */
export function CliErrorNotice({ error, cli, title, testId = "cli-error" }: { error: CliError; cli?: string; title: string; testId?: string }) {
  const { t } = useLocalization();
  const next = hasNextStep(error);
  const detail = error.message || t("app.cli.error.noMessage");
  return (
    <Notice tone="danger" title={title} testId={testId}>
      <span className="so-cli-error-code so-mono">{error.code}</span>
      <p className="so-cli-error-text">{detail}</p>
      {next && <p className="so-cli-error-next" data-testid={`${testId}-next`}>{t(`app.cli.next.${error.code}`)}</p>}
      {!next && error.hint && <p className="so-cli-error-next">{error.hint}</p>}
      {cli && <CliCommand command={cli} testId={`${testId}-cli`} />}
    </Notice>
  );
}
