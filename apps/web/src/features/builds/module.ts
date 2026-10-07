import { adminModule } from "../admin-overview/register";

export const buildModules = [
  adminModule("A11", "/admin/builds", "admin.A11", "构建", () =>
    import("./pages").then((mod) => ({ default: mod.BuildsPage })),
  ),
  adminModule("A12", "/admin/builds/:id", "admin.A12", "构建详情", () =>
    import("./pages").then((mod) => ({ default: mod.BuildDetailPage })),
  ),
];
