# DL01-W1-32 fixed review

Reviewer session `7c1526a9-b453-45cb-b1f0-25b6bae95cad`. Author content `28fd98240d28d2ac39fc2ae4452720e25e85669a`. Seal `e258101176e9b089141ac1f75ab6d3c1e62c9c02`.

Decision: `approve_setup_dependency`. `request_changes`: none.

Packet: `evidence/7c1526a9-b453-45cb-b1f0-25b6bae95cad/DL01/opensearch-fixed-review`. Decisive run `raw/` exit 0. Preserved `raw/attempt-01` (empty trailing page counted by the harness), `raw/attempt-02` and `raw/attempt-03` (cancel HTTP 200 with `resource_not_found_exception` after the Painless loop hit its statement limit).

Identity rehashed from git objects: 143 of 143 seal `SHA256SUMS` entries match, and those blobs match the content commit. Sums digest `1b3253046f4a70b9438dd0f1a1f076f30919df0ec259ab0d04c2444b1759cb85`. Image `sha256:cf05462a958be93fe2172aea2c6757debc7d3a1e4a6838d39e56af591a3dcdbb`, `linux/arm64`, `version.distribution` `opensearch`, `version.number` `3.9.0`. Author `oo-dl01w1-15` resources were absent. The review container used prefix `oo-dl01w1-32`, 1792 MiB, 1 CPU, and independent fixture values.

Approved scope is the target-preparation setup dependency: separate DBA and read-only reader, TLS, strict mapping, PIT plus `search_after` with a typed total and a deterministic compound sort, explicit refresh versus search visibility, source key-set delete reconcile, reader denials, DBA task cancel (`UBRSTIb_TmeAtJT2gRE4-A:857`, `cancelled: true`, client `task_cancelled_exception`), PIT delete, and security-audit identity. `POST /{index}/_mget` returned 403 for `indices:data/read/mget` even though the loaded role lists that action. Multi-get is outside the approved read surface.

Outside this approval: Arrow / PostgreSQL / Builder / Pack, driver compile and MSRV, matrix grade, DBX-W13, an Agent DB reconcile, and any claim of joins, foreign keys, or snapshot atomicity. The wire compatibility field `2.19.0` is not a second product.

Cleanup removed the review container, volume, and secrets. The pinned image remains. Details, raw exits, and hashes are in the packet `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, and `receipt.json`.
