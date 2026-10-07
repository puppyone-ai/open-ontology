import { AdminSurface } from "../admin-overview/surface";

export function EvaluationsPage() {
  return (
    <AdminSurface
      title="评测"
      description="套件、运行、公开域和客户域都还没有结果。"
      groups={["B10"]}
      operationIds={["B10.suites"]}
      action={{ id: "a37", label: "运行" }}
      notes={["公开、客户和 holdout 没有混在一起。这里没有保留集题目。"]}
     />
  );
}

export function EvaluationDetailPage() {
  return (
    <AdminSurface
      title="评测详情"
      description="样本数、置信区间、费用、错误归因和版本差都还没有结果。"
      groups={["B10"]}
      operationIds={["B10.run","B10.result","B10.compare","B10.privateHoldout"]}
      action={{ id: "a38", label: "比较" }}
      notes={["没有 n，也没有费用。C0 到 C3 都没有运行。","这个地址不代表已经存在的对象。"]}
     />
  );
}
