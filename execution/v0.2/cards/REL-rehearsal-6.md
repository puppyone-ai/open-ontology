## Card REL-rehearsal-6 — S6-B-01 rehearsal pass 6: final fresh-machine L1 including a scheduled cycle

TODO: S6-B-01. Worktree `v0.2/rel` at `c0826aa932c1c5bed8836a9586dd03830a1a3a60`. Report-only: no commits/tracked edits, no public registry push, no
`npm publish`, no `docker login`. Read `evidence/rel-rehearsal-3/REPORT.md`, `exact-commands.md`, `blocker.md`.
Read evidence/rel-rehearsal-5/REPORT.md and exact-commands.md first. Product code changed since pass 4 (lane G: declared source catalog → retained build), so REBUILD everything at this commit: runtime image OCI layout (both architectures), all four CLI binaries (darwin-x64 cross-built fresh), stage, npm pack, verify.

Coordinator decisions resolving pass 3's blockers (this card only):
- The fresh HOME is a new 0700 directory OUTSIDE the worktree and outside evidence (e.g. `$TMPDIR/oo-rel6-home`);
  deployment state lives under it. Import the two model files into it ONLY through the product's supported path (the
  `oo init` wizard import or the documented quickstart copy step), exactly as `tools/oo-test/l1_clean.py` does. The
  owner has authorised this test use. Never print, log, or copy their bytes anywhere else; delete that HOME at the end.
- Up to three database containers may run at once for this card (fixture source + the two lite internal databases).
- Evidence (logs, JSON, receipts) stays under `evidence/rel-rehearsal-6/`; redact nothing by hand — instead run the
  exact-byte scan of both secret files over evidence and report hits (must be 0).

Steps: throwaway local registry with pass 3's digest-pinned runtime; fresh HOME/prefix; `npm install -g` tarball;
PostgreSQL 17 fixture source (oo.lane=rel, small related tables with comments); `oo init` (import model files),
`oo up` (fresh pull), `oo build` to an unreviewed draft, the Quickstart single-owner self-review and publish (D6
mode=self), `oo share-prompt --grant-file`, Agent-style reads (`oo tables`, `oo describe`, `oo query` of a fixture
count) via the issued token, `oo status` healthy, `oo down`. Then cleanup of everything labelled oo.lane=rel, the
registry, the fresh HOME, Cargo targets.
Extra acceptance: the fixture has 2 declared FKs and table/column comments; show that facts.json, review-input.json and the published registry carry both FKs and the comments and that PKs are non-null; the isolated entity from pass 4 must be gone. Also run darwin-x64 --version/--help sweep under Rosetta and the Linux arm64 and amd64 install + help sweep in node:22-bookworm containers.
Pass 5 failed only on `oo status` healthy=false because scheduled cycles lacked the catalog; lane G pass 3 fixed it. Here, after publish, wait for at least one measured scheduled retained cycle and show `oo status` healthy=true with the cycle result data_only and the reviewed digest (bounded wait: if the stack interval is long, set the fixture stack sync interval to the shortest the product accepts and say so).
Final report: step table pass/fail with cause, query result vs fixture count, secret-scan hits, exact commands.
