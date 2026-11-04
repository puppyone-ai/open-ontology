## Card G-S2c — lane G: Builder seams for oo-load views and mTLS profiling

TODO: S2-A-02 (enables), S2-G-02. Your branch is integrated (fa49b9881). Rebase on v0.2/product-loops first.
Lane A found two seams blocking `oo build` from reaching a draft on the lite stack:
1. oo-load publishes each loaded table as a VIEW (local atomic publication); the Builder catalog (tools/pg-builder,
   crates/oo-builder catalog path) selects physical tables only. Make the catalog include the published views as
   datasets (with their underlying physical tables excluded or mapped, no double counting), keeping comments/keys/
   relationships the loader exposes; test against an Agent DB populated by oo-load (use tools/oo-test fixtures).
2. Profiling and catalog connections must present a client certificate when the Agent DB requires mTLS (lite does):
   add sslcert/sslkey (secret_ref-based, 0600 files) and CA to the Builder's connection config; never disable
   verification. Test with a TLS + client-cert PostgreSQL container.
Exit: Self-build draft generation runs against an oo-load-populated, mTLS-only Agent DB with one bounded live call
through model-egress. Work rule: smallest local workaround and keep going; commit as you go.
