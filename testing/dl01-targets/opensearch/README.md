# DL01-W1-15 OpenSearch target preparation

Pinned product: OpenSearch 3.9.0, linux/arm64, Apache-2.0. The image digest is `image-pin.json`. This is not Elasticsearch, and it is not a second product beside Elasticsearch.

The fixture is one single-node process with TLS, a setup user `oo_dl01w1_15_dba`, and a reader `oo_dl01w1_15_reader` limited to index `oo-dl01w1-15-synthetic`. The reader can map, get, search, and create or delete a PIT on that index. It cannot write documents, create indexes, read another index, or delete every PIT. The reader has no cluster permissions, so `GET /` as the reader returns 403. Product identity is read with the setup user. The reader name comes from authinfo.

`oracle/expected.json` is the typed gold for empty, missing, null, numeric, text, and multivalued fields. `scripts/journey.py` loads that file, and the DBA indexes it. The reader then opens a PIT and reads it back with `search_after` pages of 4.

Run from a checkout that already has the pinned image:

```bash
bash testing/dl01-targets/opensearch/scripts/run.sh
```

The script writes private keys and passwords under `/tmp/oo-dl01w1-15-private`, starts container `oo-dl01w1-15-opensearch` with a 1792m memory cap, runs the journey, then removes the container, the volume, and the private directory. The image is kept. No Rust compiler is started. Linux/MSRV compilation stays pending until an OpenSearch Arrow consumer exists. See `PROPOSAL.md`.

This preparation does not promote `docs/database-support/support-matrix.yaml`. The matrix cell stays `designed`. Captured consistency is B. Delete reconcile into an Agent DB is not run.
