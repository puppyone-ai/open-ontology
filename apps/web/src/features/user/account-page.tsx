import { useEffect, useState } from "react";
import { Link } from "react-router";
import { hasCapability, logoutManagementSession } from "../../api/identity-session";
import { useIdentityView, useRefreshIdentity } from "../../app/identity-context";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { UserShell } from "./user-shell";

export function AccountPage() {
  const view = useIdentityView();
  const refresh = useRefreshIdentity();
  const [logoutFailure, setLogoutFailure] = useState(false);
  useEffect(() => {
    document.title = "账户";
  }, []);
  return (
    <UserShell>
      <PageHeader title="账户" description="管理会话。" version={null} />
      {view.status === "pending" ? <StatePanel kind="loading" /> : null}
      {view.status === "absent" ? (
        <>
          <StatePanel kind="notConnected" dependency="B01" detail="当前身份尚未接入。退出和项目能力都不可用。" />
          <p className="oo-secondary">这里不回读凭证。</p>
        </>
      ) : null}
      {view.status === "anonymous" ? (
        <p>
          <Link to="/login">登录</Link>
        </p>
      ) : null}
      {view.status === "session" ? (
        <>
          <p>客户集成 not_claimed。</p>
          <p>主体 {view.session.subject}</p>
          <p>初始化状态 {view.session.installState === "initialized" ? "已初始化" : "未初始化"}</p>
          {view.session.projects.map((project) => (
            <p key={project.projectId}>
              项目 {project.projectId} 角色 {project.role} 能力 {project.capabilities.join(" ")}
            </p>
          ))}
          {view.session.projects.length === 0 ? <p>还没有项目能力。</p> : null}
          {hasCapability(view.session, "project.admin") ? (
            <p>
              <Link to="/admin">管理入口</Link>
            </p>
          ) : (
            <p>没有管理能力。</p>
          )}
          <button
            className="oo-button"
            type="button"
            onClick={() => {
              void logoutManagementSession(view.session.csrfToken).then((outcome) => {
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
          {logoutFailure ? <StatePanel kind="error" /> : null}
        </>
      ) : null}
      <p className="oo-secondary">显示偏好在页顶，只保存主题、密度和语言。</p>
    </UserShell>
  );
}
