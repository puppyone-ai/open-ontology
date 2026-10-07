import { adminModule } from "../admin-overview/register";

export const metricModules = [
  adminModule("A22", "/admin/ontology/metrics", "admin.A22", "指标", () =>
    import("./pages").then((mod) => ({ default: mod.MetricsPage })),
  ),
  adminModule("A23", "/admin/ontology/metrics/:id", "admin.A23", "指标详情", () =>
    import("./pages").then((mod) => ({ default: mod.MetricDetailPage })),
  ),
];
