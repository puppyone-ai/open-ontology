import { adminModule } from "../admin-overview/register";

export const entityTermModules = [
  adminModule("A24", "/admin/ontology/entities/:id", "admin.A24", "实体", () =>
    import("./pages").then((mod) => ({ default: mod.EntityPage })),
  ),
];
