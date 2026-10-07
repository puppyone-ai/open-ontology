export const NOT_CONNECTED = "此能力尚未接入。";
export const NO_PERMISSION = "当前身份没有执行此操作的权限。";

export function dependencyText(dependency: string): string {
  return `依赖 ${dependency}。`;
}
