import { AdminSurface } from "../admin-overview/surface";

export function ExamplesPage() {
  return (
    <AdminSurface
      title="示例"
      description="问题、SQL、执行版本和验证都还没有记录。"
      groups={["B05","B09"]}
      operationIds={["B05.examples","B09.query"]}
      action={{ id: "a25", label: "验证" }}
      notes={["未验证的示例不会进入角色包。","这里没有 SQL，也没有结果哈希。"]}
     />
  );
}
