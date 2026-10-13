# DL01-W1-22 fixed review

Reviewer session `1f05190d-5fc1-48da-84f8-7963566f8b56`. Author content `daff7345679701deaed9ff9b285d07a7b0f6c812`. Seal `7f73a9ac4aa7ba620735b2d5f9d6da30bf6c88a8`.

Decision: `approve_setup_dependency`. `request_changes`: none.

Packet: `evidence/1f05190d-5fc1-48da-84f8-7963566f8b56/DL01/clickhouse-fixed-review`. Decisive run `raw/attempt-02` exit 0. Preserved `raw/attempt-01` exit 1 (reviewer `toYYYYMM` on a string alias, code 43).

Identity rehashed from the seal commit: 441 of 441 `SHA256SUMS` entries match, sums digest `3d32cb70ef51033e2b1ef92e6fd614c613c2b8214f2f8219a1230cd5f3dbd8fe`. Image `sha256:4ea38f8345ad67dc437b41638804421ee6d21e6d8c6f794bf4b28305541de2aa`, `linux/arm64`, `version()` `26.8.21.10`. Author `oo-dl01w1-11` resources were absent. The review container used prefix `oo-dl01w1-22`, 1536 MiB, 1 CPU, and independent fixture values.

Approved scope is the target-preparation setup dependency: separate DBA and read-only reader, catalog and `FINAL` key model, decimal scale via `toDecimalString`, null versus empty, defaults, parts, statement audit, denied writes, DBA `KILL QUERY` cancel (code 394), `max_execution_time` budget (code 159), checksum stability, and rollback.

Outside this approval: Arrow / PostgreSQL / Builder / Pack, driver compile and MSRV, matrix grade, DBX-W12, full AT-SRC-03 / 07 / 10 / 11, and any future platform-reader cancel route. Lightweight delete was already done (`lag_observed` false).

Cleanup removed the review container, volume, and network. The pinned image remains. Details, raw exits, and hashes are in the packet `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, and `receipt.json`.
