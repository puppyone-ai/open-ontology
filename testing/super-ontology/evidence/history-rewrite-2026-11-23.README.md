# History rewrite before the PR (2026-11-23)

The owner approved rewriting six commit messages on the unpushed integration
branch so the PR trailer and ledger checks pass. Only these messages changed;
trees, authors, committers and dates are identical.

| Old commit | New Plan trailer | Reason |
|---|---|---|
| 78fa29366 | SO-B-S6-04 (SO-B-S6-05 moved to "Also covers") | one Plan per commit |
| a7b509cdf | SO-B-S6-10 (SO-B-S6-11, SO-B-S6-12 moved to "Also covers") | one Plan per commit |
| 39c9bb4e9 | SO-B-S6-06 (SO-B-S6-11 moved to "Also covers") | one Plan per commit |
| 2c9f4c2f3, 960535ebd, 1ff4efd06 | SO-B-S10-04 (was SO-B-L6-02) | SO-B-L6-02 is not a card id; the work is the L6 remainder (A8, A10, rebuilt images) already ledgered as SO-B-S10-04 |

Every descendant of those commits was recreated with the same tree, author,
committer, dates and message but new parents, so its SHA changed. Commits that
are ancestors of the first rewritten commit, and everything on master, keep
their SHAs.

`history-rewrite-2026-11-23.tsv` maps every changed commit, old full SHA to new
full SHA (920 rows). Cited SHAs in `testing/super-ontology/evidence/**`,
`execution/super-ontology/ledger.md` and `specs/**` were updated with the map in
the last commit of the branch (a pure reference edit). Local tags
`super-ontology/L3`, `L4`, `L5`, `L6` and `L8` were moved to the mapped commits;
`L1` and `L2` were not affected. Previous tag targets: L3 cb01cfcd4,
L4 2aa1050f6, L5 fe1b09805, L6 7a8130ae7, L8 07769fefa.

Left as they were: the v0.1 source-baseline pins `specs/open-ontology-v0.1-spec.md`,
`specs/source-baseline-revision-33.md` and `specs/sources-baseline.json`, which cite
`baseline_head` ad0c3fbc0 (now 867120bb5 in the rewritten history). They are bound
to the planning documents by SHA-256 (`specs/scripts/check_plan.py`), so editing
them would break that check; the old value stays as a historical pin.
