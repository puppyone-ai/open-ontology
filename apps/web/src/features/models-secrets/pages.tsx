import { AdminSurface } from "../admin-overview/surface";

export function ModelsPage() {
  return (
    <AdminSurface
      title="模型"
      description="Builder 和 Agent 的 provider、模型和 Secret 引用都还没有配置。"
      groups={["B11","B07"]}
      operationIds={["B11.models","B07.secretWrite"]}
      action={{ id: "a31", label: "保存" }}
      notes={["两个配置互相独立。","这里没有 Secret 输入，也不回读。"]}
     />
  );
}

export function SecretsPage() {
  return (
    <AdminSurface
      title="Secret"
      description="只写、轮换和连接测试都还没有服务端操作。"
      groups={["B07"]}
      operationIds={["B07.secretWrite","B07.secretRotate"]}
      action={{ id: "a33", label: "轮换" }}
      notes={["空输入不会覆盖已有值。","这里没有输入框，页面不回读 Secret。"]}
     />
  );
}
