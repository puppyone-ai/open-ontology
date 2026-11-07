## Card A-S4b — lane A: draft→community adapter, pinned live drafts, chain tail

TODO: S4-A-02. Rebase on v0.2/product-loops first (your S4 commits are integrated). Model configuration for live runs is
in owner-private 0600 files you are explicitly authorized to read through secret_ref (never print, copy into the repo,
argv or logs): base URL `~/.config/open-ontology/secrets/model-endpoint-9r`, key
`~/.config/open-ontology/secrets/model-key-9r-sol`, model `gpt-6.1-sol` (lane A's earlier S2f runs used them).
Keep live usage bounded (print the preflight; few calls per engine).
Deliver (keep going; commit as you go):
1. The draft → community input adapter you identified, so `oo build` feeds lane G's community/three-pass Builder path
   (not the legacy single-pass draft) and the review TUI consumes its registry.
2. Live: one-command draft on PostgreSQL 17.11, MySQL 8.4.7, Oracle 23.5 (pinned digests from testing/infra/images.pins),
   then `oo build` through review (`--review-file` accept-all) on PostgreSQL 17.11.
3. Chain tail: lane M is building the publication entry (card M-S4b: one library function + `oo-load publish`).
   Wire publish → Gateway start/reload → issue first user token (lane B's token-store) → `oo share-prompt` behind
   `build.rs::publish_reviewed_registry`; if M's entry is not on the integration branch when you get there, finish the
   token/share-prompt stages against a stub that fails closed, test resume at each stage boundary, and report exactly
   what is left. Do not edit lane M's files.
