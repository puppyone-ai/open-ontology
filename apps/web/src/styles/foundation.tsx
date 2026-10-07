import { useState } from "react";
import { foundationCopy as copy } from "./copy";

type Theme = "light" | "dark" | "system";
type Density = "compact" | "comfortable" | "roomy";

function readTheme(): Theme {
  const value = document.documentElement.dataset.theme;
  return value === "dark" || value === "system" || value === "light" ? value : "light";
}

function readDensity(): Density {
  const value = document.documentElement.dataset.density;
  return value === "compact" || value === "roomy" || value === "comfortable" ? value : "comfortable";
}

export function Foundation() {
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [density, setDensity] = useState<Density>(readDensity);

  function chooseTheme(next: Theme) {
    document.documentElement.dataset.theme = next;
    setTheme(next);
  }

  function chooseDensity(next: Density) {
    document.documentElement.dataset.density = next;
    setDensity(next);
  }

  return (
    <main className="oo-page oo-stack" lang="zh-CN">
      <header className="oo-stack">
        <h1 className="oo-title">{copy.title}</h1>
        <p className="oo-lede">{copy.lede}</p>
        <p className="oo-body">{copy.unbound}</p>
        <p className="oo-secondary">{copy.latin}</p>
      </header>

      <section aria-labelledby="type-heading" className="oo-stack">
        <h2 id="type-heading" className="oo-section-title">
          {copy.sectionType}
        </h2>
        <p className="oo-title">{copy.sectionTitleSample}</p>
        <p className="oo-body">{copy.body}</p>
        <p className="oo-secondary">{copy.secondary}</p>
        <p className="oo-meta">{copy.meta}</p>
        <p className="oo-code">
          <span className="oo-meta">{copy.codeLabel} </span>
          {copy.codeSample}
        </p>
      </section>

      <section aria-labelledby="pref-heading" className="oo-stack">
        <h2 id="pref-heading" className="oo-section-title">
          {copy.themeLegend}
        </h2>
        <fieldset className="oo-choices">
          <legend className="oo-meta">{copy.themeLegend}</legend>
          <ThemeChoice current={theme} value="light" label={copy.themeLight} onChoose={chooseTheme} />
          <ThemeChoice current={theme} value="dark" label={copy.themeDark} onChoose={chooseTheme} />
          <ThemeChoice current={theme} value="system" label={copy.themeSystem} onChoose={chooseTheme} />
        </fieldset>
        <fieldset className="oo-choices">
          <legend className="oo-meta">{copy.densityLegend}</legend>
          <DensityChoice current={density} value="compact" label={copy.densityCompact} onChoose={chooseDensity} />
          <DensityChoice current={density} value="comfortable" label={copy.densityComfortable} onChoose={chooseDensity} />
          <DensityChoice current={density} value="roomy" label={copy.densityRoomy} onChoose={chooseDensity} />
        </fieldset>
      </section>

      <section aria-labelledby="state-heading" className="oo-stack">
        <h2 id="state-heading" className="oo-section-title">
          {copy.statesLegend}
        </h2>
        <p className="oo-secondary">{copy.statesNote}</p>
        <p className="oo-note">{copy.stateUnbound}</p>
        <p className="oo-note">{copy.stateForbidden}</p>
        <p className="oo-note">{copy.stateMissing}</p>
        <p className="oo-note">{copy.stateVersion}</p>
        <p className="oo-note">{copy.stateNetwork}</p>
        <p className="oo-note">{copy.stateEmpty}</p>
      </section>

      <section aria-labelledby="rare-heading">
        <h2 id="rare-heading" className="oo-section-title">
          {copy.rareLabel}
        </h2>
        <p className="oo-body">
          {copy.rareNote}
          <span className="oo-rare" data-rare-glyph="true">
            {copy.rareGlyph}
          </span>
        </p>
      </section>

      <section aria-labelledby="sample-heading" className="oo-stack">
        <h2 id="sample-heading" className="oo-section-title">
          {copy.sectionTokens}
        </h2>
        <p className="oo-swatch" data-pair="text-on-canvas" style={{ background: "var(--oo-canvas)", color: "var(--oo-text)" }}>
          {copy.swatchText}
        </p>
        <p className="oo-swatch" data-pair="muted-on-surface" style={{ background: "var(--oo-surface)", color: "var(--oo-muted)" }}>
          {copy.swatchMuted}
        </p>
        <p className="oo-swatch" data-pair="success" style={{ background: "var(--oo-success-bg)", color: "var(--oo-success)" }}>
          {copy.swatchSuccess}
        </p>
        <p className="oo-swatch" data-pair="warning" style={{ background: "var(--oo-warning-bg)", color: "var(--oo-warning)" }}>
          {copy.swatchWarning}
        </p>
        <p className="oo-swatch" data-pair="danger" style={{ background: "var(--oo-danger-bg)", color: "var(--oo-danger)" }}>
          {copy.swatchDanger}
        </p>
        <button type="button" className="oo-button" disabled aria-describedby="button-reason">
          {copy.buttonLabel}
        </button>
        <p id="button-reason" className="oo-secondary">
          {copy.buttonReason}
        </p>
        <label className="oo-stack">
          <span>{copy.inputLabel}</span>
          <input className="oo-input" readOnly value="" aria-label={copy.inputLabel} />
        </label>
        <div className="oo-row">{copy.rowCell}</div>
        <p className="oo-motion" data-motion-sample="true">
          {copy.rowLabel}
        </p>
      </section>
    </main>
  );
}

function ThemeChoice({
  current,
  value,
  label,
  onChoose,
}: {
  current: Theme;
  value: Theme;
  label: string;
  onChoose: (value: Theme) => void;
}) {
  return (
    <label className="oo-control">
      <input type="radio" name="theme" checked={current === value} onChange={() => onChoose(value)} /> {label}
    </label>
  );
}

function DensityChoice({
  current,
  value,
  label,
  onChoose,
}: {
  current: Density;
  value: Density;
  label: string;
  onChoose: (value: Density) => void;
}) {
  return (
    <label className="oo-control">
      <input type="radio" name="density" checked={current === value} onChange={() => onChoose(value)} /> {label}
    </label>
  );
}
