import { adminModule } from "../admin-overview/register";

export const jobModules = [
  adminModule("A09", "/admin/jobs", "admin.A09", "任务", () =>
    import("./pages").then((mod) => ({ default: mod.JobsPage })),
  ),
  adminModule("A10", "/admin/jobs/:id", "admin.A10", "任务详情", () =>
    import("./pages").then((mod) => ({ default: mod.JobDetailPage })),
  ),
];
