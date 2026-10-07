import { adminModule } from "../admin-overview/register";

export const auditSessionModules = [
  adminModule("A39", "/admin/sessions", "admin.A39", "会话", () =>
    import("./pages").then((mod) => ({ default: mod.SessionsPage })),
  ),
  adminModule("A40", "/admin/sessions/:id", "admin.A40", "会话详情", () =>
    import("./pages").then((mod) => ({ default: mod.SessionDetailPage })),
  ),
  adminModule("A41", "/admin/audit", "admin.A41", "审计", () =>
    import("./pages").then((mod) => ({ default: mod.AuditPage })),
  ),
];
