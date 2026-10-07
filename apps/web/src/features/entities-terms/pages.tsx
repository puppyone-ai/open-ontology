import { AdminSurface } from "../admin-overview/surface";

export function EntityPage() {
  return (
    <AdminSurface
      title="实体"
      description="成员、键、别名、合并、否定和拆分都还没有记录。"
      groups={["B05"]}
      operationIds={["B05.entity"]}
      action={{ id: "a24", label: "保存" }}
      notes={["隐藏成员不会被列出来。","图形不能直接触发合并。金额守恒没有结果。","这个地址不代表已经存在的对象。"]}
     />
  );
}
