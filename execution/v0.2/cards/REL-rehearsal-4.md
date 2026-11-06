## Card REL-rehearsal-4 — S6-B-01 rehearsal pass 4: fresh-machine L1 through a draft and an Agent read

TODO: S6-B-01. Worktree `v0.2/rel` at `e004b3e13c7036a62b089935c20452d3cbb9242f`. Report-only: no commits/tracked edits, no public registry push, no
`npm publish`, no `docker login`. Read `evidence/rel-rehearsal-3/REPORT.md`, `exact-commands.md`, `blocker.md`.
Reuse pass 3's tarball (`f4beb3f1…`) and runtime layout if `git diff --stat <pass3-base> HEAD -- crates deploy
tools/oo-load packages` is empty; otherwise rebuild what changed.

Coordinator decisions resolving pass 3's blockers (this card only):
- The fresh HOME is a new 0700 directory OUTSIDE the worktree and outside evidence (e.g. `$TMPDIR/oo-rel4-home`);
  deployment state lives under it. Import the two model files into it ONLY through the product's supported path (the
  `oo init` wizard import or the documented quickstart copy step), exactly as `tools/oo-test/l1_clean.py` does. The
  owner has authorised this test use. Never print, log, or copy their bytes anywhere else; delete that HOME at the end.
- Up to three database containers may run at once for this card (fixture source + the two lite internal databases).
- Evidence (logs, JSON, receipts) stays under `evidence/rel-rehearsal-4/`; redact nothing by hand — instead run the
  exact-byte scan of both secret files over evidence and report hits (must be 0).

Steps: throwaway local registry with pass 3's digest-pinned runtime; fresh HOME/prefix; `npm install -g` tarball;
PostgreSQL 17 fixture source (oo.lane=rel, small related tables with comments); `oo init` (import model files),
`oo up` (fresh pull), `oo build` to an unreviewed draft, the Quickstart single-owner self-review and publish (D6
mode=self), `oo share-prompt --grant-file`, Agent-style reads (`oo tables`, `oo describe`, `oo query` of a fixture
count) via the issued token, `oo status` healthy, `oo down`. Then cleanup of everything labelled oo.lane=rel, the
registry, the fresh HOME, Cargo targets.
Final report: step table pass/fail with cause, query result vs fixture count, secret-scan hits, exact commands.
