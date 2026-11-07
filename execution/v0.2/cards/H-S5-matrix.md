## Card H-S5 — lane H: read-only proof automation and nightly matrix

TODO: S5-H-01, S5-H-02. Lane label: `h`. Spec: AT-ENG-03.
Deliver:
1. S5-H-01 T7 in oo-test: enable source-side audit in each fixture container (MySQL general log, PG
   log_statement=all, Oracle unified/standard auditing, others where available) for the reader account
   only, run T1–T2, parse the audit trail, and fail if anything other than reads/metadata/log-reader calls
   appears. Per P0 version evidence.
2. S5-H-02 nightly matrix: `oo-test matrix --all --suites T0-T4,T7,T8` (amd64 targets emulated), a CI job
   definition (ci/ and .github/workflows/nightly.yml following existing gate style) plus a local launcher
   (documented; no LaunchAgent installation), automatic downgrade report: produce a machine-readable
   evidence index that the coordinator's support-matrix generator consumes (fields: target id, version,
   capability, suite, result, run-id, sha256). Run it three times locally; report stability.
Write scope: tools/oo-test/**, testing/engines/**/seed/**, testing/engines/gold/**, ci/**, .github/workflows/nightly.yml.
