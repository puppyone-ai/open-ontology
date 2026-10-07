import { AdminSurface } from "../admin-overview/surface";
import { MetricDimensions } from "../admin-overview/blocks";

export function MetricsPage() {
  return (
    <AdminSurface
      title="指标"
      description="口径、owner、单位、锚点和认证都还没有目录。"
      groups={["B05"]}
      operationIds={["B05.metric"]}
      action={{ id: "a22", label: "刷新" }}
      notes={["这里没有指标名。"]}
     />
  );
}

export function MetricDetailPage() {
  return (
    <AdminSurface
      title="指标详情"
      description="可执行、对账、owner 认证和发布是四件不同的事，这里都没有结果。"
      groups={["B05"]}
      operationIds={["B05.metric"]}
      action={{ id: "a23", label: "认证" }}
      notes={["金额和容差保持原文，不经过 Number。","认证按钮不能改写 certified。","这个地址不代表已经存在的对象。"]}
    >
      <MetricDimensions />
    </AdminSurface>
  );
}
