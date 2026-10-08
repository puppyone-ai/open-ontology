import { useIdentityView } from "../../app/identity-context";
import { StatePanel } from "../../components/shared/state-panel";
import { AdminSurface } from "../admin-overview/surface";

function CurrentCapability() {
  const view = useIdentityView();
  if (view.status === "pending") return <StatePanel kind="loading" />;
  if (view.status !== "session") return <p>当前身份尚未接入。</p>;
  return (
    <>
      <p>客户集成 not_claimed。</p>
      <p>主体 {view.session.subject}</p>
      <p>初始化状态 {view.session.installState === "initialized" ? "已初始化" : "未初始化"}</p>
      {view.session.projects.map((project) => (
        <p key={project.projectId}>
          项目 {project.projectId} 角色 {project.role} 能力 {project.capabilities.join(" ")}
        </p>
      ))}
      {view.session.projects.length === 0 ? <p>还没有项目能力。</p> : null}
    </>
  );
}

export function RolesPage() {
  return (
    <AdminSurface
      title="角色"
      description="表、列、行过滤和配额都还没有 validate、plan 或 apply。"
      groups={["B06"]}
      operationIds={["B06.roles"]}
      action={{ id: "a26", label: "应用" }}
      notes={["浏览器不能改授权。"]}
    >
      <CurrentCapability />
    </AdminSurface>
  );
}

export function BindingsPage() {
  return (
    <AdminSurface
      title="身份绑定"
      description="用户、组和 claims 到项目角色的绑定还没有记录。"
      groups={["B01", "B06"]}
      operationIds={["B01.projectRoleBinding", "B06.bindings"]}
      action={{ id: "a27", label: "保存" }}
      notes={["这里不列出人、组或 claims。"]}
    >
      <CurrentCapability />
    </AdminSurface>
  );
}

export function PermissionPreviewPage() {
  return (
    <AdminSurface
      title="权限预览"
      description="以用户身份查看列、行和角色包的预览还没有服务端结果。"
      groups={["B06", "B05"]}
      operationIds={["B06.permissionPreview", "B05.pack"]}
      action={{ id: "a28", label: "预览" }}
      notes={["浏览器不能据此授权。"]}
    >
      <CurrentCapability />
    </AdminSurface>
  );
}
