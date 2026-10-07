import { AdminSurface } from "../admin-overview/surface";
import { SourceWizard } from "../admin-overview/blocks";

export function SourcesPage() {
  return (
    <AdminSurface
      title="数据源"
      description="连接器、模式、健康和新鲜度都还没有读数。"
      groups={["B02"]}
      operationIds={["B02.connectorMatrix","B02.sourceConfig"]}
      action={{ id: "a04", label: "添加数据源" }}
      notes={["这里没有连接清单。"]}
     />
  );
}

export function SourceWizardPage() {
  return (
    <AdminSurface
      title="新建数据源"
      description="连接、体检、选表、DBA 清单、计划和申请都还没有服务端结果。"
      groups={["B02","B07"]}
      operationIds={["B02.inspect","B02.catalog","B02.dba","B02.plan","B02.apply","B07.secretWrite"]}
      action={{ id: "a05", label: "应用" }}
      notes={["网页不对源执行 DDL。","没有可复制的 DBA 清单。","估计时长不会被写成确定数字。"]}
    >
      <SourceWizard />
    </AdminSurface>
  );
}

export function SourceDetailPage() {
  return (
    <AdminSurface
      title="数据源详情"
      description="暂停、恢复和重快照都还没有服务端操作。"
      groups={["B02","B03"]}
      operationIds={["B02.sourceConfig","B03.jobs"]}
      action={{ id: "a06", label: "暂停" }}
      notes={["这里不显示主机、库名或凭证。","这个地址不代表已经存在的对象。"]}
     />
  );
}
