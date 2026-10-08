import { useEffect } from "react";
import { Link, useSearchParams } from "react-router";
import { loginHref } from "../../api/identity-session";
import { safeReturnTo } from "../../app/safe-return";
import { useIdentityView } from "../../app/identity-context";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { AuthLayout } from "../../layouts/auth-layout";

export function LoginPage() {
  const [params] = useSearchParams();
  const returnTo = safeReturnTo(params.get("returnTo"));
  const view = useIdentityView();
  useEffect(() => {
    document.title = "登录";
  }, []);
  return (
    <AuthLayout>
      <PageHeader title="登录" description="企业身份登录。" version={null} />
      {view.status === "pending" ? <StatePanel kind="loading" /> : null}
      {view.status === "absent" ? <StatePanel kind="notConnected" dependency="B01" /> : null}
      {view.status === "anonymous" ? (
        <>
          <p className="oo-secondary">下一步离开本页，完成授权码登录。浏览器不收集密码，也不创建本地账号。</p>
          <p>
            <a href={loginHref("/account")}>开始管理身份登录</a>
          </p>
        </>
      ) : null}
      {view.status === "session" ? (
        <>
          <p>已有管理会话。客户集成 not_claimed。</p>
          <p>
            <Link to="/account">账户</Link>
          </p>
        </>
      ) : null}
      {view.status === "absent" ? <p className="oo-secondary">这里不收集密码，也不创建本地账号。</p> : null}
      {returnTo && view.status !== "session" ? <p className="oo-secondary">登录后的返回尚未接入。目标路径 {returnTo}</p> : null}
      <p>
        <Link to="/admin/login">管理入口</Link>
      </p>
    </AuthLayout>
  );
}
