import { adminModule } from "../admin-overview/register";

export const classificationModules = [
  adminModule("A19", "/admin/classifications", "admin.A19", "分类", () =>
    import("./pages").then((mod) => ({ default: mod.ClassificationsPage })),
  ),
];
