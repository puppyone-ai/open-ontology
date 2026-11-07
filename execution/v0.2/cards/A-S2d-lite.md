## Card A-S2d — lane A: lite status from the control DB, operator token, seven healthy services

TODO: S2-A-02. Rebase on v0.2/product-loops first. Coordinator corrections to card A-S2c:
- `oo status` reads last success / last failure / next due from the lite control DB (scheduler and job state),
  not from /v1/health; nearest credential expiry comes from /v1/health called with an internal operator token that
  the first `oo up` issues (lane B's issue-token / admin API, role limited to health/meta) into the 0600 secret store.
  This is an operator credential, not the user's first token (that stays S4).
- Lane G is fixing the Builder seams (catalog over oo-load views; client-cert TLS for profiling) in parallel.
Deliver: all seven lite services healthy in `oo status`, with real timestamps after one scheduled no-op cycle; then,
once lane G's fix is on the integration branch (rebase again before this step), one command from the PostgreSQL
fixture to an unreviewed draft, then MySQL 8.4 and Oracle 23. If G's fix is not integrated when you reach the draft
step, finish everything else, record the draft step as pending on G, and stop. Work rule: keep going; commit as you go.
