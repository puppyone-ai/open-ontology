import { useLocalization } from "@open-ontology/design-system/localization";
import { Button } from "@open-ontology/design-system";
import { Layers } from "lucide-react";
import { useApp } from "../../hooks/useApp";
import { useLoad } from "../../hooks/useLoad";
import { useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { selectDrives, selectOntologyVersion } from "../../model/superDriveSelectors";
import { isKnownKind, relationTarget, sortedClasses, structureApi, visibleProperties } from "../../model/structureModel";

/** The class structure of the record files on the atlas (D93): each class with its properties and the classes its relations point at. */
export function AtlasStructure({ enabled }: { enabled: boolean }) {
  const { t } = useLocalization();
  const app = useApp();
  const version = useSuperDriveSelector(selectOntologyVersion);
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d.id === b[i]?.id));
  const first = drives[0]?.id ?? null;
  const result = useLoad(() => (first ? structureApi.read(first) : Promise.resolve({ ok: true as const, value: { protocol: "", version: 0, classes: [], coverage: { record_files: 0, in_class: 0, confirmed: 0, share_in_class: 0, share_confirmed: 0 } } })), [first, version], { enabled });
  const classes = sortedClasses(result.value?.classes ?? []);
  if (classes.length === 0) return null;
  const nameOf = (kind: string, label: string) => (isKnownKind(kind) ? t(`app.structure.kind.${kind}`) : label);
  return (
    <aside className="so-atlas-structure" aria-label={t("app.structure.atlas.title")} data-testid="atlas-structure" data-classes={classes.length}>
      <p className="so-atlas-inspector-title so-row"><Layers size={14} strokeWidth={1.5} aria-hidden="true" />{t("app.structure.atlas.title")}</p>
      <ul className="so-atlas-structure-list">
        {classes.slice(0, 8).map((c) => {
          const links = [...new Set(visibleProperties(c).flatMap((p) => { const r = relationTarget(p, result.value?.classes ?? []); return r && r.kind === "class" ? [r.label] : []; }))];
          return (
            <li key={c.class_key} className="so-atlas-structure-item" data-status={c.status}>
              <span className="so-row"><span className="so-atlas-structure-dot" data-status={c.status} aria-hidden="true" /><span className="so-list-title so-truncate">{nameOf(c.kind, c.label)}</span><span className="so-weak so-atlas-structure-count">{t("app.structure.atlas.item", { count: visibleProperties(c).length })}</span></span>
              {links.length > 0 && <span className="so-atlas-structure-links so-truncate">→ {links.map((l) => (isKnownKind(l) ? t(`app.structure.kind.${l}`) : l)).join(" · ")}</span>}
            </li>
          );
        })}
      </ul>
      <Button size="compact" variant="ghost" onClick={() => app.go("ontology", { ontology: "structure" })} data-testid="atlas-structure-open">{t("app.structure.atlas.open")}</Button>
    </aside>
  );
}
