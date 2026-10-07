import { adminModule } from "../admin-overview/register";

export const exampleModules = [
  adminModule("A25", "/admin/ontology/examples", "admin.A25", "示例", () =>
    import("./pages").then((mod) => ({ default: mod.ExamplesPage })),
  ),
];
