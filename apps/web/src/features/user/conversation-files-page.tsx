import { useEffect } from "react";
import { PageHeader } from "../../components/shared/page-header";
import { StatePanel } from "../../components/shared/state-panel";
import { UserShell } from "./user-shell";

export function ConversationFilesPage() {
  useEffect(() => {
    document.title = "文件";
  }, []);
  return (
    <UserShell>
      <PageHeader title="文件" description="产出清单尚未接入。" version={null} />
      <StatePanel kind="notConnected" dependency="B07、B08" detail="这里不提供下载，也没有大小、哈希或有效期。" />
    </UserShell>
  );
}
