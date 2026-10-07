import { adminModule } from "../admin-overview/register";

export const sourceModules = [
  adminModule("A04", "/admin/sources", "admin.A04", "数据源", () =>
    import("./pages").then((mod) => ({ default: mod.SourcesPage })),
  ),
  adminModule("A05", "/admin/sources/new", "admin.A05", "新建数据源", () =>
    import("./pages").then((mod) => ({ default: mod.SourceWizardPage })),
  ),
  adminModule("A06", "/admin/sources/:id", "admin.A06", "数据源详情", () =>
    import("./pages").then((mod) => ({ default: mod.SourceDetailPage })),
  ),
];
