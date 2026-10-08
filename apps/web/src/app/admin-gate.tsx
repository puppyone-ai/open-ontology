import { useEffect, useState, type ComponentType } from "react";
import { Navigate, type RouteObject } from "react-router";
import { bindingById } from "@open-ontology/sdk-ts";
import type { ModuleRegistration } from "./module-registration";

const PUBLIC_ADMIN_PATHS = new Set(["/admin/login", "/admin/bootstrap"]);

/** schema_frozen is not a session. This stays false until getIdentityCurrent is live_verified. */
export function adminSessionReady(): boolean {
  return bindingById("getIdentityCurrent")?.state === "live_verified";
}

export function GuardedAdminModule({ module }: { module: ModuleRegistration }) {
  if (!adminSessionReady()) {
    return <Navigate to="/admin/login" replace />;
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
