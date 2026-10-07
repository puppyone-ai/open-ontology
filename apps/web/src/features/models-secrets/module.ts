import { adminModule } from "../admin-overview/register";

export const modelSecretModules = [
  adminModule("A31", "/admin/settings/models", "admin.A31", "模型", () =>
    import("./pages").then((mod) => ({ default: mod.ModelsPage })),
  ),
  adminModule("A33", "/admin/settings/secrets", "admin.A33", "Secret", () =>
    import("./pages").then((mod) => ({ default: mod.SecretsPage })),
  ),
];
