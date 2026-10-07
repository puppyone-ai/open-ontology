import { adminModule } from "./register";

export const overviewModules = [
  adminModule("A03", "/admin", "admin.A03", "总览", () =>
    import("./pages").then((mod) => ({ default: mod.OverviewPage })),
  ),
];
