import { adminModule } from "../admin-overview/register";

export const evaluationModules = [
  adminModule("A37", "/admin/evaluations", "admin.A37", "评测", () =>
    import("./pages").then((mod) => ({ default: mod.EvaluationsPage })),
  ),
  adminModule("A38", "/admin/evaluations/:id", "admin.A38", "评测详情", () =>
    import("./pages").then((mod) => ({ default: mod.EvaluationDetailPage })),
  ),
];
