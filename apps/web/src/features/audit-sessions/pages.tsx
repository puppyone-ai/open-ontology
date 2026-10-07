import { AdminSurface } from "../admin-overview/surface";

export function SessionsPage() {
  return (
    <AdminSurface
      title="会话"
      description="owner、角色、预算、租约和终止都还没有记录。"
      groups={["B08","B12"]}
      operationIds={["B08.session","B12.audit"]}
      action={{ id: "a39", label: "终止" }}
      notes={["这里没有会话清单。"]}
     />
  );
}

export function SessionDetailPage() {
  return (
    <AdminSurface
      title="会话详情"
      description="回放、SQL 关联和资源释放都还没有证据。"
      groups={["B08","B12"]}
      operationIds={["B12.replay","B08.close"]}
      action={{ id: "a40", label: "回放" }}
      notes={["S1 到 S11 没有释放证据。不能写成已关闭。","这个地址不代表已经存在的对象。"]}
     />
  );
}

export function AuditPage() {
  return (
    <AdminSurface
      title="审计"
      description="actor、query、trace 和导出来源都还没有检索结果。"
      groups={["B12"]}
      operationIds={["B12.audit"]}
      action={{ id: "a41", label: "导出" }}
      notes={["这里不列出 trace，也不提供导出文件。"]}
     />
  );
}
