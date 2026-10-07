import { useEffect } from "react";
import { Link, useSearchParams } from "react-router";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { safeReturnTo } from "../../app/safe-return";
import { AuthLayout } from "../../layouts/auth-layout";

export function LoginPage() {
  const [params] = useSearchParams();
  const returnTo = safeReturnTo(params.get("returnTo"));
  useEffect(() => {
    document.title = "登录";
  }, []);
  return (
    <AuthLayout>
      <PageHeader title="登录" description="企业身份登录尚未接入。" version={null} />
      <StatePanel kind="notConnected" dependency="B01" />
      {returnTo ? <p className="oo-secondary">登录后的返回尚未接入。目标路径 {returnTo}</p> : null}
      <p className="oo-secondary">这里不收集密码，也不创建本地账号。</p>
      <p>
        <Link to="/admin/login">管理入口</Link>
      </p>
    </AuthLayout>
  );
}
