## Card REL-final — S6-B-01 acceptance of the exact release tarball (no publishing)

TODO: S6-B-01. Worktree `v0.2/rel` reset to `origin/master` = `67e3abf333489b40df4f755c32f356b51be8899e`. Report-only: no commits/tracked edits, never
`npm publish`, never push images, never `docker login`. Outputs under `evidence/rel-final/`.

Artifact under test (do NOT rebuild anything): `/Users/redux/puppy-one/oo-v02/release-0.2.0/packed/reduxxx888-ontology-cli-0.2.0.tgz`,
sha256 `8f253cb8df3eacd6f23b0e31553dadffaad75a3c0249d4bb0d965efd9a238f99` (CI run 42408220899). Verify the hash first.
Its runtime pin is the published, signed index
`ghcr.io/puppyone-ai/open-ontology/lite@sha256:75623ff22175d7194a7f8809b24fbddd9cb45cd5c628dff0cb1af5359f783bf5`.
The package is still PRIVATE on GHCR: pulls must use this Mac's existing registry login, so point the isolated HOME's
Docker at it with `DOCKER_CONFIG=$HOME_REAL/.docker` (read-only use; do not modify that config). No local registry.

Method: exactly as `evidence/rel-rehearsal-6/REPORT.md` / `exact-commands.md` (read them first), with the CI tarball and
the GHCR pin instead of rebuilt artifacts:
1. Before: remove any local copy of the pinned runtime image by its exact reference only.
2. Fresh 0700 HOME outside evidence; `/opt/homebrew/bin/npm install -g` the tarball into a fresh prefix; every
   advertised command's `--help` and `--version`.
3. PostgreSQL 17 fixture (oo.lane=rel, 2 FKs + comments, small rows); `oo init` with the supported model-file import
   from `~/.config/open-ontology/secrets/model-endpoint-9r` and `model-key-9r-sol` (never print/copy them elsewhere);
   `oo up` must pull the pin from GHCR by itself (prove RepoDigests == pin); `oo build` draft; D6 self-review and
   publish; metadata check (FKs, comments, non-null PKs); share-prompt grant; token-bound Agent reads (fixture count);
   one unattended scheduled cycle data_only (60 s interval, as pass 6); `oo status` healthy=true; `oo down`.
4. darwin-x64 binary from the tarball: Rosetta `--version`/`--help` sweep. Linux arm64 and amd64: install the same
   tarball in `node:22-bookworm` and run the help sweep.
5. Exact-byte scan of evidence for both model files (must be 0). Clean up everything labelled oo.lane=rel and the HOME.
Final report: tarball hash check, step table pass/fail with cause, digest proof, secret-scan hits, exact commands.
