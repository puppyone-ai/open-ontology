/** Native (main process) strings: menus and dialog titles. The renderer has its own catalog. */
const CATALOG = Object.freeze({
  en: {
    "app.about": "About Super Ontology", "app.hide": "Hide Super Ontology", "app.quit": "Quit Super Ontology",
    "menu.file": "File", "menu.edit": "Edit", "menu.view": "View", "menu.window": "Window", "menu.help": "Help",
    "menu.releases": "Releases", "menu.license": "License (Apache-2.0)", "menu.logs": "Show Engine Logs in Finder",
    "menu.settings": "Settings...",
    "pick.driveRoot": "Choose the folder to use as a Drive", "pick.mountTarget": "Choose where to mount the context",
    "pick.ooExecutable": "Choose the oo executable", "pick.tokenFile": "Choose the token file of the remote engine",
    "about.detail": "Version {version}\nLicense: Apache-2.0",
  },
  "zh-Hans": {
    "app.about": "关于 Super Ontology", "app.hide": "隐藏 Super Ontology", "app.quit": "退出 Super Ontology",
    "menu.file": "文件", "menu.edit": "编辑", "menu.view": "视图", "menu.window": "窗口", "menu.help": "帮助",
    "menu.releases": "发布页", "menu.license": "许可证 (Apache-2.0)", "menu.logs": "在访达中显示引擎日志",
    "menu.settings": "设置...",
    "pick.driveRoot": "选择要用作 Drive 的文件夹", "pick.mountTarget": "选择挂载上下文的位置",
    "pick.ooExecutable": "选择 oo 可执行文件", "pick.tokenFile": "选择远端引擎的令牌文件",
    "about.detail": "版本 {version}\n许可证:Apache-2.0",
  },
});

export function resolveLanguage(setting, systemLocale = "en") {
  if (setting === "en" || setting === "zh-Hans") return setting;
  return /^zh\b/iu.test(String(systemLocale)) ? "zh-Hans" : "en";
}

export function createTranslator(getLanguage) {
  return (key, values = {}) => {
    const table = CATALOG[getLanguage()] ?? CATALOG.en;
    const text = table[key] ?? CATALOG.en[key] ?? key;
    return text.replace(/\{(\w+)\}/gu, (_match, name) => String(values[name] ?? ""));
  };
}
