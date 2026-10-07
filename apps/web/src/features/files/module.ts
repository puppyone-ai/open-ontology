import { adminModule } from "../admin-overview/register";

export const fileModules = [
  adminModule("A07", "/admin/files/new", "admin.A07", "上传文件", () =>
    import("./pages").then((mod) => ({ default: mod.FileWizardPage })),
  ),
  adminModule("A08", "/admin/files/:id", "admin.A08", "文件", () =>
    import("./pages").then((mod) => ({ default: mod.FileDetailPage })),
  ),
];
