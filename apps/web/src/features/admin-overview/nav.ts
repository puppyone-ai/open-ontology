import type { ShellNavItem } from "../../components/shared/app-shell";

export const ADMIN_NAV: readonly ShellNavItem[] = [
  { id: "A03", label: "总览", to: "/admin" },
  { id: "A04", label: "数据源", to: "/admin/sources" },
  { id: "A05", label: "新建数据源", to: "/admin/sources/new" },
  { id: "A06", label: "数据源详情", to: "/admin/sources/:id" },
  { id: "A07", label: "上传文件", to: "/admin/files/new" },
  { id: "A08", label: "文件", to: "/admin/files/:id" },
  { id: "A09", label: "任务", to: "/admin/jobs" },
  { id: "A10", label: "任务详情", to: "/admin/jobs/:id" },
];
