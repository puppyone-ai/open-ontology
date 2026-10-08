import type { ComponentType } from "react";
import { ADMIN_FEATURE_MODULES } from "../features/admin-overview/catalog";

/** G2 implements this shape in its own module.ts. G1 is the only router writer. */
export type ModuleRegistration = {
  routeId: string;
  path: string;
  capability: string;
  layout: "admin";
  title: string;
  load: () => Promise<{ default: ComponentType }>;
};

export type MountedModuleRoute = {
  path: string;
  lazy: () => Promise<{ Component: ComponentType }>;
};

/**
 * Fixed G2 snapshot d416874eee65e5f30ad9858cef4429f608066c7a.
 * The router mounts these paths behind GuardedAdminModule.
 * routesFromModules calls load(); production must not use it while the session is unbound.
 */
export const ADMIN_MODULES: readonly ModuleRegistration[] = ADMIN_FEATURE_MODULES;

export function routesFromModules(modules: readonly ModuleRegistration[]): MountedModuleRoute[] {
  return modules.map((module) => ({
    path: module.path,
    lazy: async () => {
      const loaded = await module.load();
      return { Component: loaded.default };
    },
  }));
}
