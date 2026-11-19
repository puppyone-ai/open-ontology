import type { ReactNode } from "react";
import { cx } from "./cx";

export function PageHeader({ title, description, actions, id }: { title: string; description?: string; actions?: ReactNode; id?: string }) {
  return (
    <header className="so-page-header">
      <div className="so-page-heading">
        <h1 className="so-page-title" id={id}>{title}</h1>
        {description && <p className="so-page-description">{description}</p>}
      </div>
      {actions && <div className="so-page-actions">{actions}</div>}
    </header>
  );
}

export function Section({ title, description, actions, children, className }: { title?: string; description?: string; actions?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cx("so-section", className)}>
      {(title || actions) && (
        <div className="so-section-head">
          <div>
            {title && <h2 className="so-section-title">{title}</h2>}
            {description && <p className="so-section-description">{description}</p>}
          </div>
          {actions && <div className="so-section-actions">{actions}</div>}
        </div>
      )}
      {children}
    </section>
  );
}

export function Tabs<T extends string>({ value, tabs, onChange, label }: { value: T; tabs: ReadonlyArray<{ value: T; label: string; count?: number }>; onChange: (value: T) => void; label: string }) {
  return (
    <div className="so-tabs" role="tablist" aria-label={label}>
      {tabs.map((tab, index) => (
        <button
          key={tab.value}
          type="button"
          role="tab"
          id={`so-tab-${tab.value}`}
          aria-selected={tab.value === value}
          tabIndex={tab.value === value ? 0 : -1}
          className="so-tab"
          onClick={() => onChange(tab.value)}
          onKeyDown={(event) => {
            const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
            if (!step) return;
            event.preventDefault();
            const next = tabs[(index + step + tabs.length) % tabs.length];
            if (next) { onChange(next.value); document.getElementById(`so-tab-${next.value}`)?.focus(); }
          }}
        >
          {tab.label}
          {typeof tab.count === "number" && <span className="so-tab-count">{tab.count}</span>}
        </button>
      ))}
    </div>
  );
}

/** A key/value list with the label column aligned across rows. */
export function DefinitionList({ items }: { items: ReadonlyArray<{ label: string; value: ReactNode }> }) {
  return (
    <dl className="so-definition">
      {items.map((item) => (
        <div key={item.label} className="so-definition-row">
          <dt>{item.label}</dt>
          <dd>{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}
