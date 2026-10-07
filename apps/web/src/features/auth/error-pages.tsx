import { useEffect } from "react";
import { Link } from "react-router";
import { StatePanel, type StateKind } from "../../components/shared/state-panel";
import { AuthLayout } from "../../layouts/auth-layout";

function ErrorScreen({ title, kind, detail }: { title: string; kind: StateKind; detail?: string }) {
  useEffect(() => {
    document.title = title;
  }, [title]);
  return (
    <AuthLayout>
      <h1 className="oo-title">{title}</h1>
      <StatePanel kind={kind} detail={detail} />
      <p>
        <Link to="/login">返回登录</Link>
      </p>
    </AuthLayout>
  );
}

export function ForbiddenPage() {
  return <ErrorScreen title="没有权限" kind="forbidden" />;
}

export function UnsupportedPage() {
  return <ErrorScreen title="不支持" kind="unsupported" />;
}

export function NotFoundPage() {
  return <ErrorScreen title="找不到页面" kind="notFound" />;
}

export function RouteFailure() {
  return <ErrorScreen title="页面错误" kind="error" detail="页面没有完成渲染。" />;
}
