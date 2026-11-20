/** Standard macOS menu. Commands that need the renderer go out through `onCommand` ("open-settings", "open-about"). */
export function buildMenuTemplate({ t, appName = "Super Ontology", onCommand, openReleasePage, revealLogs, isDev = false }) {
  // Role items carry their own label so they follow the app's language (the setting), not the system's.
  const role = (name, key) => ({ role: name, label: t(`native.menu.${key}`) });
  const edit = { label: t("native.menu.edit"), submenu: [role("undo", "undo"), role("redo", "redo"), { type: "separator" }, role("cut", "cut"), role("copy", "copy"), role("paste", "paste"), role("selectAll", "selectAll")] };
  return [
    {
      label: appName,
      submenu: [
        { label: t("native.app.about"), click: () => onCommand("open-about") },
        { type: "separator" },
        { label: t("native.menu.settings"), accelerator: "Cmd+,", click: () => onCommand("open-settings") },
        { type: "separator" },
        role("services", "services"), { type: "separator" },
        { label: t("native.app.hide"), role: "hide" }, role("hideOthers", "hideOthers"), role("unhide", "showAll"), { type: "separator" },
        { label: t("native.app.quit"), role: "quit" },
      ],
    },
    { label: t("native.menu.file"), submenu: [role("close", "close")] },
    edit,
    { label: t("native.menu.view"), submenu: [role("togglefullscreen", "fullscreen"), ...(isDev ? [{ type: "separator" }, { role: "reload" }, { role: "toggleDevTools" }] : [])] },
    { label: t("native.menu.window"), submenu: [role("minimize", "minimize"), role("zoom", "zoom"), { type: "separator" }, role("front", "front")] },
    { label: t("native.menu.help"), submenu: [
      { label: t("native.menu.releases"), click: () => { void openReleasePage(); } },
      { label: t("native.menu.license"), click: () => onCommand("open-about") },
      { label: t("native.menu.logs"), click: () => revealLogs() },
    ] },
  ];
}
