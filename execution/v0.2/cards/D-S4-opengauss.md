## Card D-S4 — lane D: P1 openGauss

TODO: S4-D-02. Lane label: `d`. Spec: REQ-ENG-01/04. openGauss is its own family (auth and logical
decoding fork from PostgreSQL). Requires your PG work integrated (rebase first).
Deliver: SourceEngine for openGauss 5.x/6.x (`opengauss/opengauss` or `enmotech/opengauss`, pin digests):
VersionRule table (SHA256/sm3 auth — check the driver path; reuse root PG code only where wire behavior is
identical), snapshot + C5; logical decoding capability graded from a real probe (Unsupported/NeedsSetup with
reason if not usable). T0–T2 (C5) per version.
Write scope: crates/oo-connectors/src/opengauss/**, tests, testing/engines/opengauss/** (not seed/). Prefix
`connectors/opengauss`. Report if the vendored sqlx-postgres needs a patch (shared file — do not edit).
