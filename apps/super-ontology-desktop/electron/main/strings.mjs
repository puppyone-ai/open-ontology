/** Native (main process) strings: menus and dialog titles. The renderer has its own catalog. */
const CATALOG = Object.freeze({
  en: {
    "app.about": "About Super Ontology", "app.hide": "Hide Super Ontology", "app.quit": "Quit Super Ontology",
    "menu.file": "File", "menu.edit": "Edit", "menu.view": "View", "menu.window": "Window", "menu.help": "Help",
    "menu.releases": "Releases", "menu.license": "License (Apache-2.0)", "menu.logs": "Show Engine Logs in Finder",
    "menu.settings": "Settings...",
    "pick.driveRoot": "Choose the folder to use as a Drive", "pick.mountTarget": "Choose where to mount the context",
    "pick.ooExecutable": "Choose the oo executable", "pick.tokenFile": "Choose the token file of the remote engine",
    "pick.caFile": "Choose the certificate authority file (PEM)", "pick.exportDir": "Choose where to export the ontology files",
    "about.detail": "Version {version}\nLicense: Apache-2.0",
  },
  "zh-Hans": {
    "app.about": "关于 Super Ontology", "app.hide": "隐藏 Super Ontology", "app.quit": "退出 Super Ontology",
    "menu.file": "文件", "menu.edit": "编辑", "menu.view": "视图", "menu.window": "窗口", "menu.help": "帮助",
    "menu.releases": "发布页", "menu.license": "许可证 (Apache-2.0)", "menu.logs": "在访达中显示引擎日志",
    "menu.settings": "设置...",
    "pick.driveRoot": "选择要用作 Drive 的文件夹", "pick.mountTarget": "选择挂载上下文的位置",
    "pick.ooExecutable": "选择 oo 可执行文件", "pick.tokenFile": "选择远端引擎的令牌文件",
    "pick.caFile": "选择证书颁发机构文件(PEM)", "pick.exportDir": "选择导出本体文件的位置",
    "about.detail": "版本 {version}\n许可证:Apache-2.0",
  },
  es: {
    "app.about": "Acerca de Super Ontology", "app.hide": "Ocultar Super Ontology", "app.quit": "Salir de Super Ontology",
    "menu.file": "Archivo", "menu.edit": "Edición", "menu.view": "Visualización", "menu.window": "Ventana", "menu.help": "Ayuda",
    "menu.releases": "Versiones", "menu.license": "Licencia (Apache-2.0)", "menu.logs": "Mostrar registros del motor en Finder",
    "menu.settings": "Ajustes...",
    "pick.driveRoot": "Elige la carpeta que usarás como Drive", "pick.mountTarget": "Elige dónde montar el contexto",
    "pick.ooExecutable": "Elige el ejecutable de oo", "pick.tokenFile": "Elige el archivo de token del motor remoto",
    "about.detail": "Versión {version}\nLicencia: Apache-2.0",
  },
  "pt-BR": {
    "app.about": "Sobre o Super Ontology", "app.hide": "Ocultar Super Ontology", "app.quit": "Encerrar Super Ontology",
    "menu.file": "Arquivo", "menu.edit": "Editar", "menu.view": "Visualizar", "menu.window": "Janela", "menu.help": "Ajuda",
    "menu.releases": "Versões", "menu.license": "Licença (Apache-2.0)", "menu.logs": "Mostrar logs do mecanismo no Finder",
    "menu.settings": "Ajustes...",
    "pick.driveRoot": "Escolha a pasta a usar como Drive", "pick.mountTarget": "Escolha onde montar o contexto",
    "pick.ooExecutable": "Escolha o executável do oo", "pick.tokenFile": "Escolha o arquivo de token do mecanismo remoto",
    "about.detail": "Versão {version}\nLicença: Apache-2.0",
  },
  fr: {
    "app.about": "À propos de Super Ontology", "app.hide": "Masquer Super Ontology", "app.quit": "Quitter Super Ontology",
    "menu.file": "Fichier", "menu.edit": "Édition", "menu.view": "Présentation", "menu.window": "Fenêtre", "menu.help": "Aide",
    "menu.releases": "Versions", "menu.license": "Licence (Apache-2.0)", "menu.logs": "Afficher les journaux du moteur dans le Finder",
    "menu.settings": "Réglages...",
    "pick.driveRoot": "Choisissez le dossier à utiliser comme Drive", "pick.mountTarget": "Choisissez où monter le contexte",
    "pick.ooExecutable": "Choisissez l’exécutable oo", "pick.tokenFile": "Choisissez le fichier de jeton du moteur distant",
    "about.detail": "Version {version}\nLicence : Apache-2.0",
  },
  de: {
    "app.about": "Über Super Ontology", "app.hide": "Super Ontology ausblenden", "app.quit": "Super Ontology beenden",
    "menu.file": "Datei", "menu.edit": "Bearbeiten", "menu.view": "Darstellung", "menu.window": "Fenster", "menu.help": "Hilfe",
    "menu.releases": "Versionen", "menu.license": "Lizenz (Apache-2.0)", "menu.logs": "Engine-Protokolle im Finder zeigen",
    "menu.settings": "Einstellungen...",
    "pick.driveRoot": "Ordner wählen, der als Drive dient", "pick.mountTarget": "Ziel für den Kontext wählen",
    "pick.ooExecutable": "oo-Programmdatei wählen", "pick.tokenFile": "Token-Datei der entfernten Engine wählen",
    "about.detail": "Version {version}\nLizenz: Apache-2.0",
  },
  ja: {
    "app.about": "Super Ontology について", "app.hide": "Super Ontology を隠す", "app.quit": "Super Ontology を終了",
    "menu.file": "ファイル", "menu.edit": "編集", "menu.view": "表示", "menu.window": "ウインドウ", "menu.help": "ヘルプ",
    "menu.releases": "リリース", "menu.license": "ライセンス (Apache-2.0)", "menu.logs": "エンジンのログを Finder で表示",
    "menu.settings": "設定...",
    "pick.driveRoot": "Drive として使うフォルダを選択", "pick.mountTarget": "コンテキストをマウントする場所を選択",
    "pick.ooExecutable": "oo の実行ファイルを選択", "pick.tokenFile": "リモートエンジンのトークンファイルを選択",
    "about.detail": "バージョン {version}\nライセンス: Apache-2.0",
  },
  ko: {
    "app.about": "Super Ontology 정보", "app.hide": "Super Ontology 숨기기", "app.quit": "Super Ontology 종료",
    "menu.file": "파일", "menu.edit": "편집", "menu.view": "보기", "menu.window": "윈도우", "menu.help": "도움말",
    "menu.releases": "릴리스", "menu.license": "라이선스 (Apache-2.0)", "menu.logs": "Finder에서 엔진 로그 보기",
    "menu.settings": "설정...",
    "pick.driveRoot": "Drive로 사용할 폴더 선택", "pick.mountTarget": "컨텍스트를 마운트할 위치 선택",
    "pick.ooExecutable": "oo 실행 파일 선택", "pick.tokenFile": "원격 엔진의 토큰 파일 선택",
    "about.detail": "버전 {version}\n라이선스: Apache-2.0",
  },
});

export const NATIVE_LANGUAGES = Object.freeze(Object.keys(CATALOG));

/** The language the native menus and dialogs use: the setting when it names one we have, otherwise the system's (Chinese maps to zh-Hans). */
export function resolveLanguage(setting, systemLocale = "en") {
  if (NATIVE_LANGUAGES.includes(setting)) return setting;
  const system = String(systemLocale);
  if (/^zh\b/iu.test(system)) return "zh-Hans";
  if (/^pt\b/iu.test(system)) return "pt-BR";
  const base = system.split(/[-_]/u)[0].toLowerCase();
  return NATIVE_LANGUAGES.includes(base) ? base : "en";
}

export function createTranslator(getLanguage) {
  return (key, values = {}) => {
    const table = CATALOG[getLanguage()] ?? CATALOG.en;
    const text = table[key] ?? CATALOG.en[key] ?? key;
    return text.replace(/\{(\w+)\}/gu, (_match, name) => String(values[name] ?? ""));
  };
}
