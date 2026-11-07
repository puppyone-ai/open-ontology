## Card A-S2e — lane A: lite under D14, operator token, seven healthy services, drafts

TODO: S2-A-02. Rebase on v0.2/product-loops first (your S2d commits are integrated; D14 at 773588538).
Decision D14: in the lite profile the Agent DB and control DB accept server-verified TLS + SCRAM passwords on the compose
network without client certificates (pg_hba `hostssl … scram-sha-256`); Gateway ↔ internal service mTLS is unchanged;
standard still requires client certificates. Update the lite compose/bootstrap accordingly (keep the 0600 secret store,
never weaken TLS verification of server certificates).
Deliver (work rule: keep going; commit as you go):
1. First `oo up` issues the internal operator token (health/meta-only role) through issue-token into the 0600 store;
   `oo status` shows nearest credential expiry from /v1/health with it, and scheduler/build state from the control DB.
2. All seven services healthy in `oo status` after one scheduled no-op cycle.
3. One command from the PostgreSQL fixture to an unreviewed draft (oo-load into the lite Agent DB → Self-build through
   model-egress, endpoint and key via secret_ref files ~/.config/open-ontology/secrets/model-endpoint-9r and
   model-key-9r-sol; bounded live usage), then MySQL 8.4 and Oracle 23. Lane G is adding Builder support for oo-load's
   published views; if your draft step needs it and it is not on the integration branch yet, rebase again before the
   draft step; if still absent, finish 1–2, record the draft as pending on G, and stop.
