import type { ComponentType } from "react";

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

/** Production stays empty until a fixed G2 module is appended by G1. */
export const ADMIN_MODULES: readonly ModuleRegistration[] = [];

export function routesFromModules(modules: readonly ModuleRegistration[]): MountedModuleRoute[] {
  return modules.map((module) => ({
    path: module.path,
    lazy: async () => {
      const loaded = await module.load();
      return { Component: loaded.default };
    },
  }));
}
