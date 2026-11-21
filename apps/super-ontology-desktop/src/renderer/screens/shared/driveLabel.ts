import { useLocalization } from "@open-ontology/design-system/localization";

/** The managed folder is stored as "My files" by the engine, the CLI and the app; a person reads it in their own language. */
export function useDriveLabel(): (name: string) => string {
  const { t } = useLocalization();
  return (name) => (name === "My files" ? t("app.drop.myFiles") : name);
}
