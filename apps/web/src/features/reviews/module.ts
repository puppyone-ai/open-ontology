import { adminModule } from "../admin-overview/register";

export const reviewModules = [
  adminModule("A17", "/admin/reviews", "admin.A17", "审核", () =>
    import("./pages").then((mod) => ({ default: mod.ReviewsPage })),
  ),
  adminModule("A18", "/admin/reviews/:id", "admin.A18", "审核决定", () =>
    import("./pages").then((mod) => ({ default: mod.ReviewDetailPage })),
  ),
];
