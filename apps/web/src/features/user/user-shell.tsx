import type { ReactNode } from "react";
import { AppShell, type ShellNavItem } from "../../components/shared/app-shell";

const USER_NAV: readonly ShellNavItem[] = [
  { id: "chat", label: "对话", to: "/" },
  { id: "knowledge", label: "知识", to: "/knowledge" },
  { id: "account", label: "账户", to: "/account" },
];

export function UserShell({ children }: { children: ReactNode }) {
  return (
    <AppShell entry="chat" identity={null} nav={USER_NAV} modules={[]}>
      {children}
    </AppShell>
  );
}
