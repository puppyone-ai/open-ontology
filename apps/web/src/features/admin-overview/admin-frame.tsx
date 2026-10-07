import type { ReactNode } from "react";
import { AppShell } from "../../components/shared/app-shell";
import { ADMIN_NAV } from "./nav";

export function AdminFrame({ children }: { children: ReactNode }) {
  return (
    <AppShell entry="admin" identity={null} nav={ADMIN_NAV} modules={[]}>
      {children}
    </AppShell>
  );
}
