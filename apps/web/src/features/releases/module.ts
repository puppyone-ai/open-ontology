import { adminModule } from "../admin-overview/register";

export const releaseModules = [
  adminModule("A29", "/admin/releases", "admin.A29", "发布", () =>
    import("./pages").then((mod) => ({ default: mod.ReleasesPage })),
  ),
  adminModule("A30", "/admin/releases/:id", "admin.A30", "发布详情", () =>
    import("./pages").then((mod) => ({ default: mod.ReleaseDetailPage })),
  ),
];
