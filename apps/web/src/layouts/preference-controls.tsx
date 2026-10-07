import { useEffect, useState } from "react";

export const THEME_KEY = "oo.pref.theme";
export const DENSITY_KEY = "oo.pref.density";
export const LANG_KEY = "oo.pref.lang";

const THEMES = ["light", "dark", "system"] as const;
const DENSITIES = ["compact", "comfortable", "roomy"] as const;
const LANGS = ["zh-CN", "en"] as const;

type Theme = (typeof THEMES)[number];
type Density = (typeof DENSITIES)[number];
type Lang = (typeof LANGS)[number];

function readEnum<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const value = window.localStorage.getItem(key);
    return allowed.find((item) => item === value) ?? fallback;
  } catch {
    return fallback;
  }
}

function writeEnum(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    /* preference storage is optional */
  }
}

export function PreferenceControls() {
  const [theme, setTheme] = useState<Theme>("light");
  const [density, setDensity] = useState<Density>("comfortable");
  const [lang, setLang] = useState<Lang>("zh-CN");

  useEffect(() => {
    const nextTheme = readEnum(THEME_KEY, THEMES, "light");
    const nextDensity = readEnum(DENSITY_KEY, DENSITIES, "comfortable");
    const nextLang = readEnum(LANG_KEY, LANGS, "zh-CN");
    setTheme(nextTheme);
    setDensity(nextDensity);
    setLang(nextLang);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.density = density;
    document.documentElement.lang = lang;
  }, [theme, density, lang]);

  return (
    <fieldset className="oo-choices">
      <legend className="oo-live">显示偏好</legend>
      {THEMES.map((item) => (
        <label key={item} className="oo-secondary">
          <input
            type="radio"
            name="oo-theme"
            value={item}
            checked={theme === item}
            onChange={() => {
              setTheme(item);
              writeEnum(THEME_KEY, item);
            }}
          />
          {item === "light" ? "浅色" : item === "dark" ? "深色" : "跟随系统"}
        </label>
      ))}
      {DENSITIES.map((item) => (
        <label key={item} className="oo-secondary">
          <input
            type="radio"
            name="oo-density"
            value={item}
            checked={density === item}
            onChange={() => {
              setDensity(item);
              writeEnum(DENSITY_KEY, item);
            }}
          />
          {item === "compact" ? "紧凑" : item === "comfortable" ? "舒适" : "宽松"}
        </label>
      ))}
      {LANGS.map((item) => (
        <label key={item} className="oo-secondary">
          <input
            type="radio"
            name="oo-lang"
            value={item}
            checked={lang === item}
            onChange={() => {
              setLang(item);
              writeEnum(LANG_KEY, item);
            }}
          />
          {item}
        </label>
      ))}
    </fieldset>
  );
}
