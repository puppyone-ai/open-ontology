## Card D-S4b — lane D: openGauss with the documented compatible auth mode

TODO: S4-D-02. Your D-S4 run stopped on two blockers; both have documented workarounds — use them and keep
going to T0–T2:
1. Container start: openGauss images normally require `--privileged` (label it oo.lane=d, remove it after) and a
   `GS_PASSWORD` meeting its complexity policy (upper, lower, digit, special, ≥8). Check the image docs/entrypoint
   for the exact env names of the pinned 5.0.3 / 6.0.0 images; try enmotech/opengauss if opengauss/opengauss
   will not start.
2. Auth: do not change the vendored sqlx-postgres auth code 10 handling. Configure the fixture server with
   `password_encryption_type = 1` (md5 + sha256 compatible) and create the reader with an md5 verifier, with
   pg_hba `md5` for that user; connect through the existing PostgreSQL driver path. In probe(), detect a
   server/user that is sha256-only or sm3-only and grade it NeedsSetup/Unsupported with the DBA statements
   (`ALTER SYSTEM SET password_encryption_type = 1`, re-set the reader password, pg_hba md5 line). Record this
   limitation in the VersionRule notes and testing/engines/opengauss/README.md.
Then deliver the original card: SourceEngine for openGauss (own family, reuse root PG code where wire
behavior is identical), VersionRule table, snapshot + C5, logical decoding graded from a real probe. T0–T2 (C5)
on 5.0.3 and 6.0.0. Work rule: smallest local workaround and keep going; `blocked` only if none exists.
