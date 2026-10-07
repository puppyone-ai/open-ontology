import { useEffect } from "react";
import { Link } from "react-router";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { UserShell } from "./user-shell";

export function AccountPage() {
  useEffect(() => {
    document.title = "账户";
  }, []);
  return (
    <UserShell>
      <PageHeader title="账户" description="当前身份尚未接入。退出和项目能力都不可用。" version={null} />
      <StatePanel kind="notConnected" dependency="B01" />
      <p className="oo-secondary">这里不回读凭证。</p>
      <p className="oo-secondary">显示偏好在页顶，只保存主题、密度和语言。</p>
      <p>
        <Link to="/login">登录</Link>
      </p>
    </UserShell>
  );
}
