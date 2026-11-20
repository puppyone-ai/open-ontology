// Localization checks for the Super Ontology desktop app. The approach follows the sibling desktop app's localization checker (complete catalogs, ICU
// validity, identical placeholders, no unknown or unused ids, no hard-coded user-visible text) and adds what this app needs: native catalogs,
// English left in other locales, the brand rule (D87), raw number/date formatting, and length budgets. `runChecks` returns problems instead of
// exiting so the unit tests can feed it broken fixtures.
import fs from "node:fs";
import path from "node:path";
import { IntlMessageFormat } from "intl-messageformat";
import ts from "typescript";

export const EXPECTED_LOCALES = Object.freeze(["en", "es", "pt-BR", "fr", "de", "ja", "ko", "zh-Hans"]);
export const RENDERER_NAMESPACES = Object.freeze(["app", "super-drive"]);

/** JSX attributes whose value a person reads. */
const USER_ATTRIBUTES = new Set(["alt", "aria-label", "aria-description", "aria-placeholder", "aria-roledescription", "placeholder", "title", "label"]);
/** Product names, protocol vocabulary and input examples are not translatable copy. */
const ALLOWED_LITERALS = new Set([
  "Super Ontology", "Anthropic", "Claude", "Cursor", "Codex", "Docker", "Apache License 2.0", "Alibaba Cloud OSS", "Tencent Cloud COS", "Cloudflare R2",
  "https://", "ontology/", "tables/", "db.example.test", "public.audit_log", "-----BEGIN CERTIFICATE-----", "select * from public.customers limit 10",
]);
/** Main-process object keys that become native UI text. */
const NATIVE_TEXT_PROPERTIES = new Set(["label", "title", "message", "detail", "buttonLabel", "defaultId"]);
const NATIVE_UI_FILES = ["menu.mjs", "window.mjs", "index.mjs", "super-ontology/path-picker.mjs"];
// Built from parts so this file itself passes the brand gate (check:brand).
const BRAND_PATTERN = new RegExp(["pup", "py", "\\s*-?\\s*", "one"].join(""), "iu");
/** Calls and APIs that format numbers and dates for the person; they must go through the localization layer. */
const RAW_FORMAT_PATTERN = /\.(toLocaleString|toLocaleDateString|toLocaleTimeString|toFixed)\(|\bIntl\.(NumberFormat|DateTimeFormat|RelativeTimeFormat|ListFormat|PluralRules)\b/u;

const readJson = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
const rel = (root, file) => path.relative(root, file);
/** The words of a message outside its ICU syntax: placeholders, plural headers and selectors removed, the text inside the branches kept. */
const stripSyntax = (text) => text
  .replace(/\{\s*\w+\s*,\s*(?:plural|select|selectordinal)\s*,/gu, " ")
  .replace(/(?:\b(?:one|other|few|many|zero|two)|=\d+)\s*\{/gu, " ")
  .replace(/\{\s*\w+\s*(?:,[^{}]*)?\}/gu, " ")
  .replace(/[{}#]/gu, " ");
const width = (text) => [...text].reduce((sum, ch) => sum + (/[ᄀ-ᇿ⺀-鿿가-힯＀-￯]/u.test(ch) ? 2 : 1), 0);

function walk(directory, pattern) {
  const found = [];
  if (!fs.existsSync(directory)) return found;
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name === "generated") continue;
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) found.push(...walk(full, pattern));
    else if (pattern.test(entry.name) && !/\.generated\.|\.d\.[cm]?ts$/u.test(entry.name)) found.push(full);
  }
  return found;
}

function kindOf(file) {
  if (file.endsWith(".tsx")) return ts.ScriptKind.TSX;
  if (file.endsWith(".ts")) return ts.ScriptKind.TS;
  return ts.ScriptKind.JS;
}

/** Argument names of an ICU message (the nested ones included). Throws on invalid ICU. */
export function messageArguments(message, locale) {
  const ast = new IntlMessageFormat(message, locale, undefined, { ignoreTag: false }).getAst();
  const names = new Set();
  let rich = false;
  const visit = (nodes) => {
    for (const node of nodes) {
      if (!node || typeof node !== "object") continue;
      if ([1, 2, 3, 4, 5, 6].includes(node.type)) names.add(node.value);
      if (node.type === 8) rich = true;
      if (node.options) for (const option of Object.values(node.options)) visit(option.value ?? []);
      if (Array.isArray(node.children)) visit(node.children);
    }
  };
  visit(ast);
  return { names: [...names].sort(), rich };
}

const simplePlaceholders = (text) => [...text.matchAll(/\{(\w+)\}/gu)].map((match) => match[1]).sort();

export function runChecks({ root, only = null }) {
  const problems = [];
  const fail = (message) => problems.push(message);
  const locales = path.join(root, "src/locales");
  const nativeDirectory = path.join(root, "src/shared/locales/native");
  const allowlistFile = path.join(root, "scripts/i18n/allowlist.json");
  const allowlist = fs.existsSync(allowlistFile) ? readJson(allowlistFile) : { values: {}, identical: {} };
  const stats = { renderer: 0, native: 0, short: 0 };
  const want = (name) => only === null || only.includes(name);

  // Manifest.
  const manifest = readJson(path.join(locales, "manifest.json"));
  const listed = (manifest.locales ?? []).map((entry) => entry.locale);
  if (JSON.stringify(listed) !== JSON.stringify(EXPECTED_LOCALES)) fail(`manifest: locales must be ${EXPECTED_LOCALES.join(", ")} (found ${listed.join(", ")})`);
  if (manifest.defaultLocale !== "en") fail("manifest: English must stay the fallback locale");
  if ((manifest.locales ?? []).some((entry) => entry.productionReady !== true)) fail("manifest: every listed locale must be production ready");

  // Catalogs: renderer namespaces and native.
  const catalogs = {};
  const loadCatalog = (file, label) => {
    if (!fs.existsSync(file)) { fail(`${label}: file is missing`); return {}; }
    let value;
    try { value = readJson(file); } catch (error) { fail(`${label}: not valid JSON (${error.message})`); return {}; }
    for (const [key, text] of Object.entries(value)) if (typeof text !== "string" || text.trim() === "") fail(`${label}: ${key} must be non-empty text`);
    return value;
  };
  for (const namespace of RENDERER_NAMESPACES) for (const locale of EXPECTED_LOCALES) catalogs[`${locale}:${namespace}`] = loadCatalog(path.join(locales, locale, `${namespace}.json`), `${locale}/${namespace}.json`);
  for (const locale of EXPECTED_LOCALES) catalogs[`${locale}:native`] = loadCatalog(path.join(nativeDirectory, `${locale}.json`), `native/${locale}.json`);
  if (fs.existsSync(nativeDirectory)) {
    const files = fs.readdirSync(nativeDirectory).filter((name) => name.endsWith(".json")).map((name) => name.slice(0, -5)).sort();
    if (JSON.stringify(files) !== JSON.stringify([...EXPECTED_LOCALES].sort())) fail(`native: catalog files ${files.join(", ")} do not match the locales`);
  }
  for (const directory of fs.readdirSync(locales, { withFileTypes: true }).filter((e) => e.isDirectory())) if (!EXPECTED_LOCALES.includes(directory.name)) fail(`src/locales/${directory.name}: not a listed locale`);

  const groups = [...RENDERER_NAMESPACES, "native"];
  for (const group of groups) {
    const english = catalogs[`en:${group}`];
    stats[group === "native" ? "native" : "renderer"] += Object.keys(english).length;
    if (group === "native") for (const key of Object.keys(english)) if (!key.startsWith("native.")) fail(`native/en.json: ${key} must start with native.`);
    for (const locale of EXPECTED_LOCALES) {
      const data = catalogs[`${locale}:${group}`];
      const label = group === "native" ? `native/${locale}.json` : `${locale}/${group}.json`;
      if (want("complete")) {
        for (const key of Object.keys(english)) if (!(key in data)) fail(`${label}: missing ${key}`);
        for (const key of Object.keys(data)) if (!(key in english)) fail(`${label}: extra key ${key}`);
      }
      for (const [key, text] of Object.entries(data)) {
        const source = english[key];
        if (typeof text !== "string" || source === undefined) continue;
        if (BRAND_PATTERN.test(text)) fail(`${label}: ${key} names another product (D87)`);
        if (group === "native") {
          if (/\{\s*\w+\s*,/u.test(text)) fail(`${label}: ${key} uses ICU syntax; native messages take {name} placeholders only`);
          if (simplePlaceholders(text).join(",") !== simplePlaceholders(source).join(",")) fail(`${label}: placeholders of ${key} differ`);
        } else {
          try {
            const mine = messageArguments(text, locale);
            const theirs = messageArguments(source, "en");
            if (mine.names.join(",") !== theirs.names.join(",")) fail(`${label}: placeholders of ${key} differ (${theirs.names.join(",")} vs ${mine.names.join(",")})`);
            if (mine.rich) fail(`${label}: ${key} contains a rich-text tag`);
          } catch (error) { fail(`${label}: ${key} is not valid ICU (${error.message.split("\n")[0]})`); }
        }
        // English left in another locale: identical text with real words, unless the allowlist records it for this exact English value.
        if (locale !== "en" && text === source && want("english")) {
          const words = stripSyntax(source);
          const known = allowlist.values?.[source.trim()] !== undefined || allowlist.values?.[words.trim()] !== undefined;
          const keyed = allowlist.identical?.[locale]?.[`${group}:${key}`];
          if (/\p{L}{3,}/u.test(words) && !known && keyed !== source) fail(`${label}: ${key} is still English ("${source.slice(0, 60)}")`);
        }
        // Length budget: short strings stay short, or they truncate in the controls that hold them.
        if (locale !== "en" && want("length") && group !== "native" && source.length <= 30) {
          stats.short += 1;
          const w = width(text);
          // The app's own labels have a tight budget; the shared Super Drive messages sit in wider surfaces, so theirs is looser.
          const budget = group === "app" ? source.length * 1.6 + 6 : source.length * 2.2 + 8;
          if (w > budget) fail(`${label}: ${key} is ${w} wide for English ${source.length} ("${text}")`);
        }
      }
    }
  }
  // Allowlist hygiene: no entry may outlive the English text it excuses.
  for (const [locale, entries] of Object.entries(allowlist.identical ?? {})) {
    for (const [id, englishText] of Object.entries(entries)) {
      const [group, ...rest] = id.split(":"); const key = rest.join(":");
      if (catalogs[`en:${group}`]?.[key] !== englishText) fail(`allowlist: ${locale} ${id} is stale`);
      else if (catalogs[`${locale}:${group}`]?.[key] !== englishText) fail(`allowlist: ${locale} ${id} is translated now; remove the entry`);
    }
  }

  // References from code.
  const rendererFiles = walk(path.join(root, "src/renderer"), /\.(tsx?|mjs)$/u);
  const electronFiles = walk(path.join(root, "electron"), /\.(mjs|cjs)$/u);
  const isPrefixOnly = (namespace, key, all) => Object.keys(all[`en:${namespace}`]).some((candidate) => candidate.startsWith(`${key}.`));
  const usedRenderer = { app: new Set(), "super-drive": new Set() };
  const dynamicPrefixes = [];
  const unknownRefs = [];
  const scanLiterals = (file, onLiteral, onTemplate) => {
    const sf = ts.createSourceFile(file, fs.readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true, kindOf(file));
    (function visit(node) {
      if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) onLiteral(node.text, node, sf);
      else if (ts.isTemplateExpression(node)) onTemplate(node.head.text, node, sf);
      ts.forEachChild(node, visit);
    })(sf);
  };
  const lineOf = (sf, node) => sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1;
  for (const file of rendererFiles) {
    scanLiterals(file, (text, node, sf) => {
      const match = /^(app|super-drive)\.([A-Za-z0-9_.-]+)$/u.exec(text);
      if (!match) return;
      usedRenderer[match[1]].add(match[2]);
      if (!(match[2] in catalogs[`en:${match[1]}`]) && !isPrefixOnly(match[1], match[2], catalogs)) unknownRefs.push(`${rel(root, file)}:${lineOf(sf, node)} references unknown message ${text}`);
    }, (head) => { if (/^(app|super-drive)\./u.test(head)) dynamicPrefixes.push(head); });
  }
  if (want("references")) {
    problems.push(...unknownRefs);
    const covered = (namespace, key) => usedRenderer[namespace].has(key) || dynamicPrefixes.some((prefix) => `${namespace}.${key}`.startsWith(prefix));
    for (const key of Object.keys(catalogs["en:app"])) if (!covered("app", key)) fail(`en/app.json: ${key} is unused`);
    const nativeUsed = new Set();
    for (const file of electronFiles) {
      scanLiterals(file, (text, node, sf) => {
        if (!text.startsWith("native.") || !/^native\.[\w.]+$/u.test(text)) return;
        nativeUsed.add(text);
        if (!(text in catalogs["en:native"])) fail(`${rel(root, file)}:${lineOf(sf, node)} references unknown native message ${text}`);
      }, () => {});
    }
    // `role(name, key)` in menu.mjs builds native.menu.<key> from a literal key; accept those.
    const menuSource = fs.existsSync(path.join(root, "electron/main/menu.mjs")) ? fs.readFileSync(path.join(root, "electron/main/menu.mjs"), "utf8") : "";
    for (const match of menuSource.matchAll(/role\("\w+",\s*"(\w+)"\)/gu)) nativeUsed.add(`native.menu.${match[1]}`);
    for (const key of Object.keys(catalogs["en:native"])) if (!nativeUsed.has(key)) fail(`native/en.json: ${key} is unused`);
  }

  // Hard-coded user-visible text.
  if (want("hardcoded")) {
    for (const file of rendererFiles.filter((f) => f.endsWith(".tsx"))) {
      const sf = ts.createSourceFile(file, fs.readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
      const check = (value, node, kind) => {
        const text = value.replace(/\s+/gu, " ").trim();
        if (!text || !/\p{L}{2,}/u.test(text) || ALLOWED_LITERALS.has(text)) return;
        fail(`${rel(root, file)}:${lineOf(sf, node)} hard-coded ${kind} ${JSON.stringify(text.slice(0, 70))}`);
      };
      (function visit(node) {
        if (ts.isJsxText(node)) check(node.text, node, "text");
        else if (ts.isJsxAttribute(node) && USER_ATTRIBUTES.has(node.name.getText()) && node.initializer) {
          const init = node.initializer;
          const literal = ts.isStringLiteral(init) ? init : ts.isJsxExpression(init) && init.expression && (ts.isStringLiteral(init.expression) || ts.isNoSubstitutionTemplateLiteral(init.expression)) ? init.expression : null;
          if (literal) check(literal.text, node, `${node.name.getText()} attribute`);
        } else if (ts.isJsxExpression(node) && node.expression && (ts.isStringLiteral(node.expression) || ts.isNoSubstitutionTemplateLiteral(node.expression)) && (ts.isJsxElement(node.parent) || ts.isJsxFragment(node.parent))) check(node.expression.text, node, "text expression");
        ts.forEachChild(node, visit);
      })(sf);
    }
    for (const relative of NATIVE_UI_FILES) {
      const file = path.join(root, "electron/main", relative);
      if (!fs.existsSync(file)) continue;
      const sf = ts.createSourceFile(file, fs.readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
      (function visit(node) {
        if (ts.isPropertyAssignment(node) && NATIVE_TEXT_PROPERTIES.has(node.name.getText()) && (ts.isStringLiteral(node.initializer) || ts.isNoSubstitutionTemplateLiteral(node.initializer))) {
          const text = node.initializer.text.trim();
          if (/\p{L}{2,}/u.test(text) && !ALLOWED_LITERALS.has(text)) fail(`${rel(root, file)}:${lineOf(sf, node)} hard-coded native ${node.name.getText()} ${JSON.stringify(text.slice(0, 70))}`);
        }
        ts.forEachChild(node, visit);
      })(sf);
    }
  }

  // Numbers, dates and sizes go through the localization layer.
  if (want("formatting")) {
    for (const file of rendererFiles) {
      const text = fs.readFileSync(file, "utf8");
      text.split("\n").forEach((line, index) => {
        if (RAW_FORMAT_PATTERN.test(line) && !/\/\/ i18n-ok/u.test(line)) fail(`${rel(root, file)}:${index + 1} formats a number or date outside the localization layer`);
      });
    }
  }
  return { problems, stats };
}
