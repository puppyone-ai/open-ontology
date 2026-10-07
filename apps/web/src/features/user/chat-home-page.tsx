import { useEffect } from "react";
import { Link } from "react-router";
import { ChatComposer } from "../../components/shared/chat-composer";
import { NOT_CONNECTED } from "../../components/shared/copy";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { UserShell } from "./user-shell";

/** Unavailable U02 surface. `/` does not mount this while the client has no session. */
export function ChatHomePage() {
  useEffect(() => {
    document.title = "对话";
  }, []);
  return (
    <UserShell>
      <div className="oo-chat-column">
        <PageHeader title="对话" description="当前没有已验证的身份，也不能选择角色。" version={null} />
        <StatePanel kind="notConnected" dependency="B01、B08" detail="停止本轮和结束会话都尚未接入。" />
        <p className="oo-secondary">这里没有历史记录，也没有可发送的问题。</p>
        <ChatComposer state="disabled" disabledReason={NOT_CONNECTED} onSend={() => undefined} onStop={() => undefined} onEndSession={() => undefined} />
        <p>
          <Link to="/login">登录</Link>
        </p>
      </div>
    </UserShell>
  );
}
