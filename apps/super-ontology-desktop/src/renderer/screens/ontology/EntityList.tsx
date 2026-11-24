import { useEffect, useState } from "react";
import { useDisplayName } from "../../hooks/useDisplayName";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, EmptyState, Notice, Select, Skeleton } from "@open-ontology/design-system";
import { Network, Search } from "lucide-react";
import { ErrorText } from "../shared/ErrorText";
import { useLoad } from "../../hooks/useLoad";
import { ENTITY_TYPE_VALUES } from "../../../shared/super-ontology/docs-api.generated";
import { ontologyApi } from "../../model/superDriveOntologyApi";
import { useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { selectOntologyVersion } from "../../model/superDriveSelectors";
import { isEngineReady } from "../../model/superDriveStateModel";

export function EntityList({ onOpen }: { onOpen: (key: string) => void }) {
  const { t } = useLocalization();
  const shown = useDisplayName();
  const ready = useSuperDriveSelector((s) => isEngineReady(s.engine));
  const version = useSuperDriveSelector(selectOntologyVersion);
  const [query, setQuery] = useState("");
  const [type, setType] = useState("");
  const [applied, setApplied] = useState("");
  // The list follows the typing after a short pause; Enter applies it at once.
  useEffect(() => {
    const timer = window.setTimeout(() => setApplied(query.trim()), 250);
    return () => window.clearTimeout(timer);
  }, [query]);
  const result = useLoad(() => ontologyApi.searchEntities({ q: applied, type: type || undefined, limit: 100 }), [applied, type, ready, version], { enabled: ready });
  // People, organizations and projects first: the documents the engine also lists as entities are noise on a first look.
  const items = [...(result.value?.items ?? [])].sort((a, b) => Number(a.type === "Document") - Number(b.type === "Document"));

  return (
    <div className="so-stack">
      <form className="so-toolbar" role="search" onSubmit={(event) => { event.preventDefault(); setApplied(query.trim()); }}>
        <div className="so-search">
          <Search size={16} strokeWidth={1.5} className="so-search-icon so-icon" aria-hidden="true" />
          <input className="so-input" type="search" data-testid="entity-search" value={query} onChange={(e) => { setQuery(e.target.value); if (e.target.value === "") setApplied(""); }} placeholder={t("app.ontology.search")} aria-label={t("app.ontology.search")} />
        </div>
        <div style={{ width: 180 }}><Select data-testid="entity-type-filter" label={t("app.ontology.type")} value={type} onChange={(e) => setType(e.target.value)} labelHidden>
          <option value="">{t("app.ontology.type.all")}</option>
          {ENTITY_TYPE_VALUES.map((value) => <option key={value} value={value}>{t(`super-drive.entity.type.${value}`)}</option>)}
        </Select></div>
      </form>
      {result.status === "error" && !result.value ? (
        <Notice tone="danger" title={t("app.common.loadFailed")} action={<Button size="compact" onClick={result.reload}>{t("app.common.retry")}</Button>}><ErrorText error={result.error} /></Notice>
      ) : result.status === "loading" && !result.value ? (
        <Card className="so-list" aria-busy="true">{Array.from({ length: 5 }, (_, i) => <div key={i} className="so-list-row"><Skeleton width={32} height={32} radius={6} /><span className="so-list-text"><Skeleton width={160} height={13} /><Skeleton width={100} height={11} /></span></div>)}</Card>
      ) : items.length === 0 ? (
        <Card><EmptyState icon={Network} title={applied || type ? t("app.ontology.none.title") : t("app.ontology.empty.title")} description={applied || type ? t("app.ontology.none.body") : t("app.ontology.empty.body")} /></Card>
      ) : (
        <Card className="so-list" data-testid="entity-list">
          {items.map((entity) => (
            <button key={entity.id} type="button" className="so-list-row" onClick={() => onOpen(entity.key)} data-testid="entity-row" data-name={entity.name} data-type={entity.type}>
              <span className="so-list-lead so-weak" aria-hidden="true">{shown(entity.name).slice(0, 1).toUpperCase()}</span>
              <span className="so-list-text">
                <span className="so-list-title so-truncate">{shown(entity.name)}</span>
                <span className="so-list-sub so-truncate">{t(`super-drive.entity.type.${entity.type}`)}{entity.matched_alias ? ` · ${t("app.ontology.alias", { alias: entity.matched_alias })}` : ""}</span>
              </span>
              <span className="so-weak">{t("app.ontology.facts", { count: entity.counts?.facts ?? 0 })}</span>
            </button>
          ))}
        </Card>
      )}
    </div>
  );
}
