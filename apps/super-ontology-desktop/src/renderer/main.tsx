import "@open-ontology/design-system/styles.css";
import "./app.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { LocalizationProvider } from "@open-ontology/design-system/localization/react";
import { App } from "./App";
import { installDevBridge } from "./dev-bridge";
import { createLocaleClient, loadCatalogBundle } from "./i18n";

async function boot() {
  if (import.meta.env.DEV) installDevBridge();
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
