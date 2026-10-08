import { useEffect, useState, type FormEvent } from "react";
import { Link } from "react-router";
import { consumeBootstrapToken, loginHref } from "../../api/identity-session";
import { useIdentityView, useRefreshIdentity } from "../../app/identity-context";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { AuthLayout } from "../../layouts/auth-layout";

export function BootstrapPage() {
  const view = useIdentityView();
  const refresh = useRefreshIdentity();
  const [token, setToken] = useState("");
  const [failure, setFailure] = useState(false);
  useEffect(() => {
    document.title = "初始化";
  }, []);
  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (view.status !== "session") {
      return;
    }
    setFailure(false);
    const outcome = await consumeBootstrapToken(view.session.csrfToken, token);
    setToken("");
    if (outcome !== "consumed") {
      setFailure(true);
      return;
    }
    refresh();
  }
  return (
    <AuthLayout>
      <PageHeader title="初始化" description="一次性初始化。" version={null} />
      {view.status === "pending" ? <StatePanel kind="loading" /> : null}
      {view.status === "absent" ? (
        <>
          <StatePanel kind="notConnected" dependency="B01" detail="浏览器不能在这里设置密码。" />
          <p>
            <Link to="/admin/login">返回管理登录</Link>
          </p>
        </>
      ) : null}
      {view.status === "anonymous" ? (
        <>
          <p className="oo-secondary">浏览器不能在这里设置密码。</p>
          <p>
            <a href={loginHref("/admin/bootstrap")}>开始管理身份登录</a>
          </p>
        </>
      ) : null}
      {view.status === "session" && view.session.installState === "uninitialized" ? (
        <form onSubmit={(event) => void onSubmit(event)}>
          <p className="oo-secondary">浏览器不能在这里设置密码。客户集成 not_claimed。</p>
          <label>
            引导令牌
            <input
              className="oo-input"
              name="bootstrap-token"
              type="password"
              autoComplete="off"
              value={token}
              onChange={(event) => setToken(event.target.value)}
            />
          </label>
          <button className="oo-button" type="submit">提交初始化</button>
          {failure ? <StatePanel kind="error" /> : null}
        </form>
      ) : null}
      {view.status === "session" && view.session.installState === "initialized" ? (
        <>
          <p>已经初始化。客户集成 not_claimed。</p>
          <p>
            <Link to="/admin">管理入口</Link>
          </p>
        </>
      ) : null}
    </AuthLayout>
  );
}
