import { useMemo, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Icon } from "@open-ontology/design-system";
import { ChevronRight, Database, File, Folder, Package } from "lucide-react";
import type { AtlasData, AtlasMember } from "./model";

/**
 * The keyboard and screen reader equivalent of the graph (D88): every group and member the picture shows, as a plain tree of buttons,
 * with the same search. Rows past `PAGE` per group wait behind "Show more", so ten thousand members stay a short document.
 */
const PAGE = 100;
const SOURCE_ICON = { files: File, folder: Folder, bucket: Package, database: Database } as const;
export const COUNT_KEY = { type: "app.atlas.count.entities", drive: "app.atlas.count.files", db: "app.atlas.count.tables" } as const;

export function AtlasList({ data, query, onOpen }: { data: AtlasData; query: string; onOpen: (member: AtlasMember) => void }) {
  const { t } = useLocalization();
  // A picture of a few groups (a database alone, say) shows its members at once: there is nothing else to look at.
  const [open, setOpen] = useState<ReadonlySet<string>>(() => new Set(data.groups.length <= 3 ? data.groups.map((group) => group.id) : []));
  const [relationLimit, setRelationLimit] = useState(PAGE);
  const [limits, setLimits] = useState<Readonly<Record<string, number>>>({});
  const needle = query.trim().toLocaleLowerCase();

  const tree = useMemo(() => {
    const links = new Map<string, number>();
    for (const link of data.links) { links.set(link.a, (links.get(link.a) ?? 0) + 1); links.set(link.b, (links.get(link.b) ?? 0) + 1); }
    const byGroup = new Map<string, AtlasMember[]>();
    for (const member of data.members) {
      if (needle && !member.label.toLocaleLowerCase().includes(needle)) continue;
      const list = byGroup.get(member.group);
      if (list) list.push(member); else byGroup.set(member.group, [member]);
    }
    return { links, byGroup };
  }, [data, needle]);

  const labels = useMemo(() => new Map(data.members.map((member) => [member.id, member.label])), [data.members]);
  const relations = useMemo(() => data.links.filter((link) => !needle || [link.a, link.b].some((id) => (labels.get(id) ?? "").toLocaleLowerCase().includes(needle))), [data.links, labels, needle]);
  const groups = data.groups.filter((group) => !needle || tree.byGroup.has(group.id) || group.label.toLocaleLowerCase().includes(needle));
  const toggle = (id: string) => setOpen((current) => { const next = new Set(current); if (!next.delete(id)) next.add(id); return next; });

  return (
    <ul className="so-atlas-list" data-testid="atlas-list" aria-label={t("app.atlas.list")}>
      {groups.map((group) => {
        const members = tree.byGroup.get(group.id) ?? [];
        const expanded = open.has(group.id) || (needle.length > 0 && members.length > 0);
        const limit = limits[group.id] ?? PAGE;
        return (
          <li key={group.id} className="so-atlas-list-group" data-tone={group.tone}>
            <button type="button" className="so-atlas-list-head" aria-expanded={expanded} onClick={() => toggle(group.id)} data-testid="atlas-list-group" data-group={group.id}>
              <Icon icon={ChevronRight} className="so-atlas-list-chevron" />
              {group.source ? (() => { const Glyph = SOURCE_ICON[group.source]; return <Glyph className="so-atlas-kind" size={12} strokeWidth={1.5} absoluteStrokeWidth aria-hidden="true" focusable="false" />; })() : <span className="so-atlas-swatch" aria-hidden="true" />}
              <span className="so-atlas-list-name so-truncate">{group.label}</span>
              <span className="so-weak so-atlas-count">{t(COUNT_KEY[group.kind], { count: members.length })}</span>
            </button>
            {expanded && (
              <ul className="so-atlas-list-members">
                {members.slice(0, limit).map((member) => (
                  <li key={member.id}>
                    <button type="button" className="so-atlas-list-row" onClick={() => onOpen(member)} data-testid="atlas-list-member" data-name={member.label}>
                      <span className="so-atlas-list-name so-truncate">{member.label}</span>
                      {(tree.links.get(member.id) ?? 0) > 0 && <span className="so-weak so-atlas-count">{t("app.atlas.linked", { count: tree.links.get(member.id) ?? 0 })}</span>}
                    </button>
                  </li>
                ))}
                {members.length > limit && (
                  <li><Button size="compact" variant="ghost" onClick={() => setLimits((c) => ({ ...c, [group.id]: limit + PAGE }))}>{t("app.atlas.more", { count: members.length - limit })}</Button></li>
                )}
              </ul>
            )}
          </li>
        );
      })}
      {relations.length > 0 && (
        <li className="so-atlas-list-group" data-testid="atlas-relations">
          <p className="so-atlas-relations-head"><span>{t("app.atlas.relations")}</span><span className="so-weak so-atlas-count">{relations.length}</span></p>
          <ul className="so-atlas-list-members so-atlas-relations">
            {relations.slice(0, relationLimit).map((link) => {
              const [from, to] = link.from === link.b ? [link.b, link.a] : [link.a, link.b];
              return (
                <li key={`${link.kind}|${link.a}|${link.b}`} className="so-atlas-relation" data-testid="atlas-relation">
                  <span className="so-truncate">{labels.get(from) ?? from}</span>
                  <span className="so-atlas-arrow" aria-hidden="true">{link.kind === "link" ? "—" : "→"}</span>
                  <span className="so-truncate">{labels.get(to) ?? to}</span>
                </li>
              );
            })}
            {relations.length > relationLimit && (
              <li><Button size="compact" variant="ghost" onClick={() => setRelationLimit((n) => n + PAGE)}>{t("app.atlas.more", { count: relations.length - relationLimit })}</Button></li>
            )}
          </ul>
        </li>
      )}
    </ul>
  );
}
