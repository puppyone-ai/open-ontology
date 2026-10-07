import type { ReactNode } from "react";
import { PreferenceControls } from "./preference-controls";

export function AuthLayout({ children }: { children: ReactNode }) {
  return (
    <main className="oo-auth">
      <section className="oo-auth-card">
        <p className="oo-brand">Open Ontology</p>
        {children}
        <PreferenceControls />
      </section>
    </main>
  );
}
