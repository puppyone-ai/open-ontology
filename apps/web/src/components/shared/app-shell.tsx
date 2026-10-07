import { useState, type ReactNode } from "react";
import { Link } from "react-router";
import type { ModuleRegistration } from "../../app/module-registration";
import { NOT_CONNECTED } from "./copy";
import { PreferenceControls } from "../../layouts/preference-controls";

export type ShellEntry = "chat" | "admin" | "auth";

export type ShellIdentity = {
  label: string;
  projectId: string;
  role: string;
};

export type ShellNavItem = {
  id: string;
  label: string;
  to: string;
};

export function AppShell({
  entry,
  identity,
  nav,
  modules,
  children,
}: {
  entry: ShellEntry;
  identity: ShellIdentity | null;
  nav: readonly ShellNavItem[];
  modules: readonly ModuleRegistration[];
  children: ReactNode;
}) {
  const [collapsed, setCollapsed] = useState(false);
  const [drawer, setDrawer] = useState(false);
  const showAdmin = entry === "admin" && identity !== null;
  return (
    <div className="oo-shell" data-entry={entry} data-collapsed={collapsed ? "true" : "false"}>
      <aside className="oo-sidebar" data-open={drawer ? "true" : "false"} data-collapsed={collapsed ? "true" : "false"}>
        <p className="oo-brand">{entry === "admin" ? "管理" : "对话"}</p>
        {identity ? (
          <p className="oo-secondary">
            {identity.label} · {identity.projectId} · {identity.role}
          </p>
        ) : (
          <p className="oo-secondary">{NOT_CONNECTED}</p>
        )}
        <ul className="oo-nav">
          {nav.map((item) => (
            <li key={item.id}>
              <Link to={item.to}>{item.label}</Link>
            </li>
          ))}
          {showAdmin
            ? modules.map((module) => (
                <li key={module.routeId}>
                  <Link to={module.path}>{module.title}</Link>
                </li>
              ))
            : null}
        </ul>
        <button type="button" className="oo-button oo-button-ghost" onClick={() => setCollapsed((value) => !value)}>
          {collapsed ? "展开导航" : "折叠导航"}
        </button>
      </aside>
      <div className="oo-main">
        <div className="oo-topbar">
          <button type="button" className="oo-button oo-button-ghost" onClick={() => setDrawer((value) => !value)}>
            菜单
          </button>
          <PreferenceControls />
        </div>
        <div className="oo-page">{children}</div>
      </div>
    </div>
  );
}
