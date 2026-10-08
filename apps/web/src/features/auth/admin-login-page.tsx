import { useEffect } from "react";
import { Link } from "react-router";
import { loginHref } from "../../api/identity-session";
import { useIdentityView } from "../../app/identity-context";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { AuthLayout } from "../../layouts/auth-layout";

export function AdminLoginPage() {
  const view = useIdentityView();
  useEffect(() => {
    document.title = "管理登录";
  }, []);
  return (
    <AuthLayout>
      <PageHeader title="管理登录" description="当前没有已验证的管理身份。" version={null} />
      {view.status === "pending" ? <StatePanel kind="loading" /> : null}
      {view.status === "absent" ? <StatePanel kind="notConnected" dependency="B01" /> : null}
      {view.status === "anonymous" || view.status === "session" ? (
        <p>
          <a href={loginHref("/admin/bootstrap")}>开始管理身份登录</a>
        </p>
      ) : null}
      <p className="oo-secondary">普通身份不会因为打开这个地址而获得管理能力。</p>
      <p className="oo-secondary">这里不收集密码，也不创建本地账号。</p>
      <p>
        <Link to="/login">返回登录</Link>
      </p>
    </AuthLayout>
  );
}
