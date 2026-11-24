import "@open-ontology/design-system/styles.css";
import "./app.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { LocalizationProvider } from "@open-ontology/design-system/localization/react";
import { setClipboardWriter } from "@open-ontology/design-system";
import { copyText } from "./model/format";
import { App } from "./App";
import { installDevBridge } from "./dev-bridge";
import { adoptShellLanguage, createLocaleClient, loadCatalogBundle } from "./i18n";

/** A truncated name shows its full text as a tooltip on hover: set once, lazily, for every `.so-truncate` element. */
function installTruncationTitles() {
  document.addEventListener("mouseover", (event) => {
    const target = event.target instanceof Element ? event.target.closest<HTMLElement>(".so-truncate") : null;
    if (!target || target.title !== "") return;
    if (target.scrollWidth > target.clientWidth + 1) target.title = target.textContent ?? "";
  }, { passive: true });
}

async function boot() {
  installTruncationTitles();
  setClipboardWriter(copyText);
  if (import.meta.env.DEV) installDevBridge();
  await adoptShellLanguage();
  const client = createLocaleClient();
  const state = client.getState();
  const bundle = await loadCatalogBundle(state.locale);
  const root = document.getElementById("root");
  if (!root) throw new Error("The renderer root is missing.");
  createRoot(root).render(
    <StrictMode>
      <LocalizationProvider initialState={state} initialCatalog={bundle.catalog} fallbackCatalog={bundle.fallbackCatalog} loadCatalog={loadCatalogBundle} client={client}>
        <App />
      </LocalizationProvider>
    </StrictMode>,
  );
}

void boot();
