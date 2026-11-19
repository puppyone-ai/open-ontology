import { useLocalization } from "@open-ontology/design-system/localization";
import { Icon, IconButton, Tooltip, cx } from "@open-ontology/design-system";
import { Bot, FolderOpen, House, Network, PanelLeftClose, PanelLeftOpen, Settings, type LucideIcon } from "lucide-react";
import { useApp, type Section } from "../hooks/useApp";
import { useSuperDriveSelector } from "../model/SuperDriveProvider";
import { selectDriftedShareCount, selectPendingReviewTotal } from "../model/superDriveSelectors";
import { AppMark } from "./AppMark";

const ITEMS: ReadonlyArray<{ section: Section; icon: LucideIcon; label: string }> = [
  { section: "home", icon: House, label: "app.nav.home" },
  { section: "sources", icon: FolderOpen, label: "app.nav.sources" },
  { section: "ontology", icon: Network, label: "app.nav.ontology" },
  { section: "agents", icon: Bot, label: "app.nav.agents" },
];

export function Nav() {
  const { t } = useLocalization();
  const app = useApp();
  const pending = useSuperDriveSelector(selectPendingReviewTotal);
  const drifted = useSuperDriveSelector(selectDriftedShareCount);
  const collapsed = app.navCollapsed;
  const count = (section: Section) => (section === "ontology" ? pending : section === "agents" ? drifted : 0);

  const row = (section: Section, glyph: LucideIcon, labelId: string) => {
    const label = t(labelId);
    const n = count(section);
    const button = (
      <button
        key={section}
        type="button"
        className="so-nav-item"
        aria-current={app.section === section ? "page" : undefined}
        aria-label={collapsed ? (n > 0 ? t("app.nav.withCount", { label, count: n }) : label) : undefined}
        onClick={() => app.go(section)}
        data-testid={`nav-${section}`}
      >
        <Icon icon={glyph} size={20} />
        {!collapsed && <span className="so-nav-label">{label}</span>}
        {n > 0 && <span className="so-nav-count" data-collapsed={collapsed ? "true" : undefined} aria-label={collapsed ? undefined : t("app.nav.countLabel", { count: n })}>{n > 99 ? "99+" : n}</span>}
      </button>
    );
    return collapsed ? <Tooltip key={section} label={label}>{button}</Tooltip> : button;
  };

  return (
    <nav className="so-nav" data-collapsed={collapsed ? "true" : undefined} aria-label={t("app.nav.label")}>
      <div className="so-nav-head" data-drag-region>
        <span className="so-nav-brand"><AppMark size={20} />{!collapsed && <span className="so-nav-name">Super Ontology</span>}</span>
      </div>
      <div className="so-nav-list">{ITEMS.map((item) => row(item.section, item.icon, item.label))}</div>
      <div className="so-nav-foot">
        {row("settings", Settings, "app.nav.settings")}
        <div className={cx("so-nav-toggle")}>
          <IconButton icon={collapsed ? PanelLeftOpen : PanelLeftClose} label={collapsed ? t("app.nav.expand") : t("app.nav.collapse")} size="compact" onClick={app.toggleNav} aria-expanded={!collapsed} />
        </div>
      </div>
    </nav>
  );
}
