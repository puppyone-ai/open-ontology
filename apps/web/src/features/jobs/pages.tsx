import { AdminSurface } from "../admin-overview/surface";
import { JobRecord } from "../admin-overview/blocks";

export function JobsPage() {
  return (
    <AdminSurface
      title="任务"
      description="任务筛选、进度和失败都还没有服务端记录。"
      groups={["B03"]}
      operationIds={["B03.jobs"]}
      action={{ id: "a09", label: "刷新" }}
      notes={["这里没有任务清单。未知不是 0。"]}
     />
  );
}

export function JobDetailPage() {
  return (
    <AdminSurface
      title="任务详情"
      description="检查点、事件、取消和重试都还没有服务端操作。"
      groups={["B03"]}
      operationIds={["B03.checkpoints","B03.events","B03.cancel","B03.retry"]}
      action={{ id: "a10", label: "重试" }}
      notes={["关闭页面不会取消任务。EOF 不是完成。","这个地址不代表已经存在的对象。"]}
    >
      <JobRecord />
    </AdminSurface>
  );
}
