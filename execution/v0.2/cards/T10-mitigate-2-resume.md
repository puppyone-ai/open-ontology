## Card T10-mitigate-2-resume — continue the chunk-consistent mitigation (S6-H-02; D24)

Branch `v0.2/t10` (committed; never delete this card). Your previous run of `T10-mitigate-2.md` was cut off by a
model-relay outage (502), not by a test result. Your uncommitted edits are still in the worktree; run `git diff`,
review them, and continue exactly from `execution/v0.2/cards/T10-mitigate-2.md` (all decisions D-a..D-d stand,
all tests, live T1/T2 on PG 17 and MySQL 8.4, full T10 re-measure on Oracle 23, MySQL 8.4, PostgreSQL 17, docs).
Commit in small logical steps as the owner, no agent trailers, trailer `Plan: S6-H-02`, so a future outage loses
little. Same host rules: unrelated containers are idle; never stop, inspect beyond `docker stats`, modify or name them.
