import { AdminSurface } from "../admin-overview/surface";
import { RecoveryWizard } from "../admin-overview/blocks";

export function OperationsPage() {
  return (
    <AdminSurface
      title="运维"
      description="健康、延迟、积压、配额和紧急停止都还没有服务端状态。"
      groups={["B12"]}
      operationIds={["B12.health","B12.emergency"]}
      action={{ id: "a42", label: "紧急停止" }}
      notes={["按钮不能表示资源已经消失。","gateway.health.get 不是这一页的 B12 绑定。"]}
     />
  );
}

export function RecoveryPage() {
  return (
    <AdminSurface
      title="恢复"
      description="备份、升级、回滚和恢复证据都还没有服务端任务。"
      groups={["B12"]}
      operationIds={["B12.backup","B12.upgrade","B12.recovery"]}
      action={{ id: "a43", label: "恢复" }}
      notes={["没有已验证的备份可供选择。"]}
    >
      <RecoveryWizard />
    </AdminSurface>
  );
}
