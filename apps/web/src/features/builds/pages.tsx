import { AdminSurface } from "../admin-overview/surface";
import { BuildStages } from "../admin-overview/blocks";

export function BuildsPage() {
  return (
    <AdminSurface
      title="构建"
      description="Self-build 的来源、模型版本和范围都还没有运行。"
      groups={["B04"]}
      operationIds={["B04.builder"]}
      action={{ id: "a11", label: "运行构建" }}
      notes={["没有模型调用。失败或未运行都不会产生审核或发布。"]}
     />
  );
}

export function BuildDetailPage() {
  return (
    <AdminSurface
      title="构建详情"
      description="S0 到 S10、候选、缓存、用量和费用都还没有记录。"
      groups={["B04","B03"]}
      operationIds={["B04.cards","B04.candidates","B03.jobs"]}
      action={{ id: "a12", label: "重试" }}
      notes={["没有用量，也没有费用。模型自评置信度没有数值。","这个地址不代表已经存在的对象。"]}
    >
      <BuildStages />
    </AdminSurface>
  );
}
