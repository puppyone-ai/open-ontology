import { AdminSurface } from "../admin-overview/surface";

export function OntologyPage() {
  return (
    <AdminSurface
      title="本体"
      description="数据集、术语、实体和指标都还不能按领域检索。"
      groups={["B04","B05"]}
      operationIds={["B04.candidates","B05.terms"]}
      action={{ id: "a13", label: "刷新" }}
      notes={["这里不列出对象名或版本。"]}
     />
  );
}

export function DatasetPage() {
  return (
    <AdminSurface
      title="数据集"
      description="字段、主键、外键、画像和三层结果都还没有读数。"
      groups={["B04"]}
      operationIds={["B04.candidates"]}
      action={{ id: "a14", label: "刷新" }}
      notes={["估计行数不会被写成确定数字。","这个地址不代表已经存在的对象。"]}
     />
  );
}

export function RelationsPage() {
  return (
    <AdminSurface
      title="关系"
      description="关系列表和局部图都还没有授权对象。"
      groups={["B04"]}
      operationIds={["B04.candidates"]}
      action={{ id: "a15", label: "刷新" }}
      notes={["图形不能直接合并或拆分。"]}
     />
  );
}

export function TermsPage() {
  return (
    <AdminSurface
      title="术语"
      description="定义、同义词、映射和生命周期都还没有记录。"
      groups={["B05"]}
      operationIds={["B05.terms"]}
      action={{ id: "a16", label: "保存" }}
      notes={["这里没有术语，也没有 owner。"]}
     />
  );
}
