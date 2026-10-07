import { useEffect } from "react";
import { Link } from "react-router";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { AuthLayout } from "../../layouts/auth-layout";

export function BootstrapPage() {
  useEffect(() => {
    document.title = "初始化";
  }, []);
  return (
    <AuthLayout>
      <PageHeader title="初始化" description="一次性初始化还没有接入。" version={null} />
      <StatePanel kind="notConnected" dependency="B01" detail="浏览器不能在这里设置密码。" />
      <p>
        <Link to="/admin/login">返回管理登录</Link>
      </p>
    </AuthLayout>
  );
}
