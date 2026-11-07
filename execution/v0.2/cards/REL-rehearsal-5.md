## Card REL-rehearsal-5 — S6-B-01 rehearsal pass 5: full fresh-machine L1 after the catalog-metadata fix

TODO: S6-B-01. Worktree `v0.2/rel` at `2bf8059196b48a1558c029987a68b8cf911a1ee5`. Report-only: no commits/tracked edits, no public registry push, no
`npm publish`, no `docker login`. Read `evidence/rel-rehearsal-3/REPORT.md`, `exact-commands.md`, `blocker.md`.
Read evidence/rel-rehearsal-4/REPORT.md and exact-commands.md first. Product code changed since pass 4 (lane G: declared source catalog → retained build), so REBUILD everything at this commit: runtime image OCI layout (both architectures), all four CLI binaries (darwin-x64 cross-built fresh), stage, npm pack, verify.

Coordinator decisions resolving pass 3's blockers (this card only):
- The fresh HOME is a new 0700 directory OUTSIDE the worktree and outside evidence (e.g. `$TMPDIR/oo-rel5-home`);
  deployment state lives under it. Import the two model files into it ONLY through the product's supported path (the
  `oo init` wizard import or the documented quickstart copy step), exactly as `tools/oo-test/l1_clean.py` does. The
  owner has authorised this test use. Never print, log, or copy their bytes anywhere else; delete that HOME at the end.
- Up to three database containers may run at once for this card (fixture source + the two lite internal databases).
- Evidence (logs, JSON, receipts) stays under `evidence/rel-rehearsal-5/`; redact nothing by hand — instead run the
  exact-byte scan of both secret files over evidence and report hits (must be 0).

Steps: throwaway local registry with pass 3's digest-pinned runtime; fresh HOME/prefix; `npm install -g` tarball;
PostgreSQL 17 fixture source (oo.lane=rel, small related tables with comments); `oo init` (import model files),
`oo up` (fresh pull), `oo build` to an unreviewed draft, the Quickstart single-owner self-review and publish (D6
mode=self), `oo share-prompt --grant-file`, Agent-style reads (`oo tables`, `oo describe`, `oo query` of a fixture
count) via the issued token, `oo status` healthy, `oo down`. Then cleanup of everything labelled oo.lane=rel, the
registry, the fresh HOME, Cargo targets.
Extra acceptance: the fixture has 2 declared FKs and table/column comments; show that facts.json, review-input.json and the published registry carry both FKs and the comments and that PKs are non-null; the isolated entity from pass 4 must be gone. Also run darwin-x64 --version/--help sweep under Rosetta and the Linux arm64 and amd64 install + help sweep in node:22-bookworm containers.
Final report: step table pass/fail with cause, query result vs fixture count, secret-scan hits, exact commands.
