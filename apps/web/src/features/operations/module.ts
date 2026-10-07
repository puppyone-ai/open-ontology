import { adminModule } from "../admin-overview/register";

export const operationModules = [
  adminModule("A42", "/admin/operations", "admin.A42", "运维", () =>
    import("./pages").then((mod) => ({ default: mod.OperationsPage })),
  ),
  adminModule("A43", "/admin/operations/recovery", "admin.A43", "恢复", () =>
    import("./pages").then((mod) => ({ default: mod.RecoveryPage })),
  ),
];
