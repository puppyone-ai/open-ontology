import { adminModule } from "../admin-overview/register";

export const enumCalendarModules = [
  adminModule("A20", "/admin/ontology/enums/:id", "admin.A20", "码表", () =>
    import("./pages").then((mod) => ({ default: mod.EnumPage })),
  ),
  adminModule("A21", "/admin/ontology/calendars", "admin.A21", "业务日历", () =>
    import("./pages").then((mod) => ({ default: mod.CalendarPage })),
  ),
];
