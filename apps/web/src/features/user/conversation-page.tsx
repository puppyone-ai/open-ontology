import { useEffect } from "react";
import { Link } from "react-router";
import { ChatComposer } from "../../components/shared/chat-composer";
import { NOT_CONNECTED } from "../../components/shared/copy";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { UserShell } from "./user-shell";

export function ConversationPage() {
  useEffect(() => {
    document.title = "对话";
  }, []);
  return (
    <UserShell>
      <div className="oo-chat-column">
        <PageHeader title="对话" description="这个地址不代表已经存在的对话。" version={null} />
        <StatePanel kind="notConnected" dependency="B08、B09" detail="没有消息、引用或结果。" />
        <ChatComposer state="disabled" disabledReason={NOT_CONNECTED} onSend={() => undefined} onStop={() => undefined} onEndSession={() => undefined} />
        <p>
          <Link to="files">文件</Link>
        </p>
      </div>
    </UserShell>
  );
}
