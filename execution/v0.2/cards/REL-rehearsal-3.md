## Card REL-rehearsal-3 — S6-B-01 rehearsal pass 3: fresh darwin-x64 and oo build to a draft

TODO: S6-B-01. Worktree `v0.2/rel` reset to `cd2a18eded7ecc3866564c3f66c807fbd7bf00a9`. Report-only: no commits, no tracked-file edits, no public registry
push, no `npm publish`, no `docker login`. Outputs under `evidence/rel-rehearsal-3/`. Read
`evidence/rel-rehearsal-2/REPORT.md` and `exact-commands.md` first and reuse the method (including the Compose plugin
discovery setup for isolated HOMEs).

Secrets: the live model is reachable only through owner-private 0600 files
`~/.config/open-ontology/secrets/model-endpoint-9r` and `~/.config/open-ontology/secrets/model-key-9r-sol`. Never print,
log, copy into evidence, or pass their contents on a command line. Reference them only the way
`tools/oo-test/l1_clean.py` does (stack.yaml `secret_ref` entries resolved from files the CLI reads; see lines ~1040-1060
and its --model-endpoint/--model-key handling). After the run, grep every evidence file and captured log for the exact
bytes of both files and report the hit count (must be 0) without showing the bytes.

1. Rebuild the darwin-x64 binary fresh at this commit (cross-build as in pass 1) and all other platforms only if their
   inputs changed since pass 2's base; restage all four, `npm pack`, verify; reuse pass 2's runtime layout if
   `git diff --stat <pass2-base> HEAD -- deploy crates tools/oo-load` is empty, else rebuild it.
2. Throwaway registry as before; fresh macOS arm64 HOME/prefix and fresh image state; `npm install -g` tarball.
3. Throwaway PostgreSQL 17 source (label oo.lane=rel) seeded with a small fixture schema (a few related tables with
   comments; fixture data only). `oo init` from a stack.yaml with the model secret_refs, `oo up` (fresh pull),
   `oo build` to an unreviewed draft, then whatever review/publish/self-review step the Quickstart (docs/guide) gives for
   a single-owner deployment (mode=self, D6), `oo share-prompt --grant-file`, one Agent-style read through the CLI
   (`oo tables`, `oo query` of a fixture count), `oo status` healthy, `oo down`.
4. darwin-x64: under Rosetta, `--version`, `--help` and every subcommand `--help` from the fresh binary.
5. Cleanup as before (everything labelled oo.lane=rel, registry, builders, Cargo targets). Keep evidence.
Final report: tarball identity, step table with pass/fail and cause, secret-scan hit count, exact commands (secrets
referenced by path only).
