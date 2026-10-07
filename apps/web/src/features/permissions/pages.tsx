import { AdminSurface } from "../admin-overview/surface";

export function RolesPage() {
  return (
    <AdminSurface
      title="角色"
      description="表、列、行过滤和配额都还没有 validate、plan 或 apply。"
      groups={["B06"]}
      operationIds={["B06.roles"]}
      action={{ id: "a26", label: "应用" }}
      notes={["浏览器不能改授权。"]}
     />
  );
}

export function BindingsPage() {
  return (
    <AdminSurface
      title="身份绑定"
      description="用户、组和 claims 到项目角色的绑定还没有记录。"
      groups={["B01","B06"]}
      operationIds={["B01.projectRoleBinding","B06.bindings"]}
      action={{ id: "a27", label: "保存" }}
      notes={["这里不列出人、组或 claims。"]}
     />
  );
}

export function PermissionPreviewPage() {
  return (
    <AdminSurface
      title="权限预览"
      description="以用户身份查看列、行和角色包的预览还没有服务端结果。"
      groups={["B06","B05"]}
      operationIds={["B06.permissionPreview","B05.pack"]}
      action={{ id: "a28", label: "预览" }}
      notes={["浏览器不能据此授权。"]}
     />
  );
}
