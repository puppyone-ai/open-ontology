import { adminModule } from "../admin-overview/register";

export const settingsModules = [
  adminModule("A32", "/admin/settings/agent", "admin.A32", "Agent", () =>
    import("./pages").then((mod) => ({ default: mod.AgentSettingsPage })),
  ),
  adminModule("A34", "/admin/settings/deployment", "admin.A34", "部署", () =>
    import("./pages").then((mod) => ({ default: mod.DeploymentPage })),
  ),
  adminModule("A35", "/admin/clients", "admin.A35", "外部客户端", () =>
    import("./pages").then((mod) => ({ default: mod.ClientsPage })),
  ),
  adminModule("A36", "/admin/channels", "admin.A36", "渠道", () =>
    import("./pages").then((mod) => ({ default: mod.ChannelsPage })),
  ),
];
