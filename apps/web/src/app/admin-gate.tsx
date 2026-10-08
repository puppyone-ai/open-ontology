import { useEffect, useState, type ComponentType } from "react";
import { Navigate, type RouteObject } from "react-router";
import { adminGateDecision } from "../api/identity-session";
import type { IdentityView } from "../api/identity-session";
import { useIdentityView } from "./identity-context";
import type { ModuleRegistration } from "./module-registration";

const PUBLIC_ADMIN_PATHS = new Set(["/admin/login", "/admin/bootstrap"]);

/** Catalog state is not a session. The default view is absent, so this stays false. */
export function adminSessionReady(view: IdentityView = { status: "absent" }): boolean {
  return adminGateDecision(view) === "allow";
}

export function GuardedAdminModule({ module }: { module: ModuleRegistration }) {
  const decision = adminGateDecision(useIdentityView());
  if (decision === "wait") {
    return null;
  }
  if (decision === "login") {
    return <Navigate to="/admin/login" replace />;
  }
  if (decision === "bootstrap") {
    return <Navigate to="/admin/bootstrap" replace />;
  }
  if (decision === "forbidden") {
    return <Navigate to="/forbidden" replace />;
  }
  return <AdminModuleHost module={module} />;
}

function AdminModuleHost({ module }: { module: ModuleRegistration }) {
  const [Page, setPage] = useState<ComponentType | null>(null);
  useEffect(() => {
    let cancelled = false;
    void module.load().then((loaded) => {
      if (!cancelled) setPage(() => loaded.default);
    });
    return () => {
      cancelled = true;
    };
  }, [module]);
  if (!Page) return null;
  return <Page />;
}

export function guardedAdminRoutes(modules: readonly ModuleRegistration[]): RouteObject[] {
  return modules.map((module) => {
    if (PUBLIC_ADMIN_PATHS.has(module.path)) {
      throw new Error(`admin module must not replace ${module.path}`);
    }
    return {
      path: module.path,
      element: <GuardedAdminModule module={module} />,
    };
  });
}
