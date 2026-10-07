import type { ComponentType } from "react";
import type { ModuleRegistration } from "../../app/module-registration";

export function adminModule(
  routeId: string,
  path: string,
  capability: string,
  title: string,
  load: () => Promise<{ default: ComponentType }>,
): ModuleRegistration {
  return { routeId, path, capability, layout: "admin", title, load };
}
