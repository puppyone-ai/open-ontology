## Card M-S6e — lane M: license texts, then the Quickstart run

TODO: S6-M-01, S6-M-02. Rebase on v0.2/product-loops first (keep your commits). Commit trailers: `Plan:` only — no
Co-Authored-By (owner instruction).
1. npm staging refuses eight crates whose packages ship no license file (flatbuffers, objc2-core-foundation,
   objc2-system-configuration, russh, russh-cryptovec, russh-util, tiberius-macros, and the eighth in
   `ci/s6d-release-evidence.json`). For each: check the crate's upstream repository at the exact version's tag for a
   license file and vendor that text under `packages/cli/licenses/third-party/<crate>-<version>/` with its source URL;
   only if upstream has none, use the canonical SPDX text of the declared license expression with the crate's authors
   as the copyright line, marked "canonical SPDX text; crate ships none". The collector reads these overrides; the
   staging check stays strict for everything else.
2. Quickstart run: build the npm pack (all platforms you can build locally; Darwin x64 stays CI-only — say so), install
   it into a fresh HOME, and follow docs/guide/quickstart.md literally for PostgreSQL 17.11 to a working Agent query.
   Model configuration (explicitly authorised; read only via secret_ref, never print or copy): base URL
   `~/.config/open-ontology/secrets/model-endpoint-9r`, key `~/.config/open-ontology/secrets/model-key-9r-sol`, model
   `gpt-6.1-sol`. Fix every doc step that does not work as written; report product gaps with evidence.
