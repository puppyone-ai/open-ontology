import { useState, type ReactNode } from "react";
import { logoutManagementSession, type ManagementSession } from "../../api/identity-session";
import { useIdentityView, useRefreshIdentity } from "../../app/identity-context";
import { AppShell, type ShellIdentity } from "../../components/shared/app-shell";
import { StatePanel } from "../../components/shared/state-panel";
import { ADMIN_NAV } from "./nav";

function shellIdentity(session: ManagementSession): ShellIdentity {
  const project = session.projects.find((item) => item.capabilities.includes("project.admin")) ?? session.projects[0];
  return {
    label: session.subject,
    projectId: project?.projectId ?? "没有项目",
    role: project?.role ?? "没有角色",
  };
}

export function AdminFrame({ children }: { children: ReactNode }) {
  const view = useIdentityView();
  const refresh = useRefreshIdentity();
  const [logoutFailure, setLogoutFailure] = useState(false);
  const identity = view.status === "session" ? shellIdentity(view.session) : null;
  const csrfToken = view.status === "session" ? view.session.csrfToken : null;
  return (
    <AppShell entry="admin" identity={identity} nav={ADMIN_NAV} modules={[]}>
      {csrfToken ? (
        <p>
          <button
            className="oo-button"
            type="button"
            onClick={() => {
              void logoutManagementSession(csrfToken).then((outcome) => {
                if (outcome === "revoked") {
                  setLogoutFailure(false);
                  refresh();
                  return;
                }
                setLogoutFailure(true);
              });
            }}
          >
            退出
          </button>
        </p>
      ) : null}
      {logoutFailure ? <StatePanel kind="error" /> : null}
      {children}
    </AppShell>
  );
}
