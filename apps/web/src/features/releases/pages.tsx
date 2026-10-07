import { AdminSurface } from "../admin-overview/surface";
import { ReleaseGates } from "../admin-overview/blocks";

export function ReleasesPage() {
  return (
    <AdminSurface
      title="发布"
      description="active、候选和回滚历史都还没有服务端指针。"
      groups={["B05"]}
      operationIds={["B05.publish","B05.pack"]}
      action={{ id: "a29", label: "刷新" }}
      notes={["这里没有当前版本，也不能回滚。"]}
     />
  );
}

export function ReleaseDetailPage() {
  return (
    <AdminSurface
      title="发布详情"
      description="候选 seal、门禁、角色包、副本屏障和 CAS 都还没有运行。"
      groups={["B05","B03"]}
      operationIds={["B05.gates","B05.publish","B03.jobs"]}
      action={{ id: "a30", label: "发布" }}
      notes={["未运行不是通过。","没有候选，不能发布，也不能强制通过安全门禁。","这个地址不代表已经存在的对象。"]}
    >
      <ReleaseGates />
    </AdminSurface>
  );
}
