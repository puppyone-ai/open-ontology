import { adminModule } from "../admin-overview/register";

export const permissionModules = [
  adminModule("A26", "/admin/permissions/roles", "admin.A26", "角色", () =>
    import("./pages").then((mod) => ({ default: mod.RolesPage })),
  ),
  adminModule("A27", "/admin/permissions/bindings", "admin.A27", "身份绑定", () =>
    import("./pages").then((mod) => ({ default: mod.BindingsPage })),
  ),
  adminModule("A28", "/admin/permissions/preview", "admin.A28", "权限预览", () =>
    import("./pages").then((mod) => ({ default: mod.PermissionPreviewPage })),
  ),
];
