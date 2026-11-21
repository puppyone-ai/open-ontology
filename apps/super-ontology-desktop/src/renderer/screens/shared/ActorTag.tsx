import { Badge } from "@open-ontology/design-system";
import { useLocalization } from "@open-ontology/design-system/localization";
import type { EngineActor } from "../../../shared/super-ontology/bridge";

/** "via CLI", "via agent qa-bot", "in app": who made a change. A client the engine could not name (`local`) and changes it made itself show nothing. */
export function ActorTag({ actor }: { actor: EngineActor | null | undefined }) {
  const { t } = useLocalization();
  if (!actor || actor.kind === "local") return null;
  const label = actor.kind === "agent" ? t("app.actor.agent", { name: actor.name ?? "" }) : t(`app.actor.${actor.kind}`);
  return <span data-testid="actor-tag" data-actor={actor.kind} data-name={actor.name}><Badge tone="neutral">{label}</Badge></span>;
}
