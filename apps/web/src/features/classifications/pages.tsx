import { AdminSurface } from "../admin-overview/surface";

export function ClassificationsPage() {
  return (
    <AdminSurface
      title="分类"
      description="列上的 pii、semantic_type、来源和可见角色都还没有清单。"
      groups={["B04","B06"]}
      operationIds={["B04.classifications", "B06.roles", "listClassifications", "previewClassifications", "confirmClassifications"]}
      action={{ id: "a19", label: "批量确认" }}
      notes={["unknown 不是 0。","没有批量范围，也没有授权。未分类内容继续隐藏。"]}
     />
  );
}
