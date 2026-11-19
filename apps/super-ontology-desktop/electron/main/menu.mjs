/** Standard macOS menu. Commands that need the renderer go out through `onCommand` ("open-settings", "open-about"). */
export function buildMenuTemplate({ t, appName = "Super Ontology", onCommand, openReleasePage, revealLogs, isDev = false }) {
  const edit = { label: t("menu.edit"), submenu: [{ role: "undo" }, { role: "redo" }, { type: "separator" }, { role: "cut" }, { role: "copy" }, { role: "paste" }, { role: "selectAll" }] };
  return [
    {
      label: appName,
      submenu: [
        { label: t("app.about"), click: () => onCommand("open-about") },
        { type: "separator" },
        { label: t("menu.settings"), accelerator: "Cmd+,", click: () => onCommand("open-settings") },
        { type: "separator" },
        { role: "services" }, { type: "separator" },
        { label: t("app.hide"), role: "hide" }, { role: "hideOthers" }, { role: "unhide" }, { type: "separator" },
        { label: t("app.quit"), role: "quit" },
      ],
    },
    { label: t("menu.file"), submenu: [{ role: "close" }] },
    edit,
    { label: t("menu.view"), submenu: [{ role: "togglefullscreen" }, ...(isDev ? [{ type: "separator" }, { role: "reload" }, { role: "toggleDevTools" }] : [])] },
    { label: t("menu.window"), submenu: [{ role: "minimize" }, { role: "zoom" }, { type: "separator" }, { role: "front" }] },
    { label: t("menu.help"), submenu: [
      { label: t("menu.releases"), click: () => { void openReleasePage(); } },
      { label: t("menu.license"), click: () => onCommand("open-about") },
      { label: t("menu.logs"), click: () => revealLogs() },
    ] },
  ];
}
