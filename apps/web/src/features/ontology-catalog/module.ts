import { adminModule } from "../admin-overview/register";

export const ontologyModules = [
  adminModule("A13", "/admin/ontology", "admin.A13", "本体", () =>
    import("./pages").then((mod) => ({ default: mod.OntologyPage })),
  ),
  adminModule("A14", "/admin/ontology/datasets/:id", "admin.A14", "数据集", () =>
    import("./pages").then((mod) => ({ default: mod.DatasetPage })),
  ),
  adminModule("A15", "/admin/ontology/relations", "admin.A15", "关系", () =>
    import("./pages").then((mod) => ({ default: mod.RelationsPage })),
  ),
  adminModule("A16", "/admin/ontology/terms", "admin.A16", "术语", () =>
    import("./pages").then((mod) => ({ default: mod.TermsPage })),
  ),
];
