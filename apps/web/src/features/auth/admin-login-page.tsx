import { useEffect } from "react";
import { Link } from "react-router";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { AuthLayout } from "../../layouts/auth-layout";

export function AdminLoginPage() {
  useEffect(() => {
    document.title = "管理登录";
  }, []);
  return (
    <AuthLayout>
      <PageHeader title="管理登录" description="当前没有已验证的管理身份。" version={null} />
      <StatePanel kind="notConnected" dependency="B01" />
      <p className="oo-secondary">普通身份不会因为打开这个地址而获得管理能力。</p>
      <p>
        <Link to="/login">返回登录</Link>
      </p>
    </AuthLayout>
  );
}
