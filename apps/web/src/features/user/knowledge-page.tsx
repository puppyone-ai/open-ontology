import { useEffect } from "react";
import { Link } from "react-router";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { UserShell } from "./user-shell";

export function KnowledgePage() {
  useEffect(() => {
    document.title = "知识";
  }, []);
  return (
    <UserShell>
      <PageHeader title="知识" description="当前角色的发布知识尚未接入。" version={null} />
      <StatePanel kind="notConnected" dependency="B05、B09" />
      <p className="oo-secondary">这里不列出目录、表、术语、指标、示例或关系。</p>
      <p>
        <Link to="/login">登录</Link>
      </p>
    </UserShell>
  );
}
