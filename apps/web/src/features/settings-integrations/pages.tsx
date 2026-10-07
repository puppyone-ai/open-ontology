import { AdminSurface } from "../admin-overview/surface";

export function AgentSettingsPage() {
  return (
    <AdminSurface
      title="Agent"
      description="harness、预算、追加说明和渠道都还没有配置。"
      groups={["B11"]}
      operationIds={["B11.harness","B11.channels"]}
      action={{ id: "a32", label: "保存" }}
      notes={["这里不能上传可执行代码。"]}
     />
  );
}

export function DeploymentPage() {
  return (
    <AdminSurface
      title="部署"
      description="对象存储、保留、时区和 Git 计划都还没有配置。"
      groups={["B11"]}
      operationIds={["B11.deployment","B11.git","B11.config"]}
      action={{ id: "a34", label: "应用" }}
      notes={["网页不能对源执行 DDL，也不能在线构建。"]}
     />
  );
}

export function ClientsPage() {
  return (
    <AdminSurface
      title="外部客户端"
      description="MCP、OAuth、令牌交换和撤销都还不能注册。"
      groups={["B06","B11"]}
      operationIds={["B06.oauth","B06.clientToken","B06.revoke","B11.plugin"]}
      action={{ id: "a35", label: "撤销" }}
      notes={["这一页不发放令牌。"]}
     />
  );
}

export function ChannelsPage() {
  return (
    <AdminSurface
      title="渠道"
      description="验签、映射和测试都还没有服务端状态。"
      groups={["B11"]}
      operationIds={["B11.channels"]}
      action={{ id: "a36", label: "测试" }}
      notes={["测试不会发出真实消息。"]}
     />
  );
}
