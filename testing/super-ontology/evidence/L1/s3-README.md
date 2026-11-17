# docs-rustfs and docs-minio (B-S4-04)

`tools/oo-test/oo-test fast --layer L1 --cells docs-rustfs,docs-minio --run-id <id>` runs one labelled container per cell
(`oo.lane=super-ontology-be1`, volumes labelled too, removed afterwards) and the real store tests of the S3 backend and of
the remote download. `s3-docs-rustfs.txt` and `s3-docs-minio.txt` hold the commands and the test lines.

| cell | image (pinned by digest) | s3_rustfs (7) | download (3) |
|---|---|---|---|
| docs-rustfs | `rustfs/rustfs` 1.0.6 | 7/7 | 3/3, 200 MiB in 13.2 s, process memory +32.8 MiB |
| docs-minio | `pgsty/minio` RELEASE.2026-08-04 (community fork; `minio/minio` left Docker Hub in September 2026) | 7/7 | 3/3, 200 MiB in 12.9 s, +32.5 MiB |

What the two stores answered (the capability probe, `s3_rustfs::the_probe_reports_what_the_store_does`, both identical):
`If-Match` on a ranged read is honoured (a stale tag is a 412, which the backend maps to `Changed`), the ETag of a single
part object is the MD5 of the content, the ETag of a multipart object is not (36 characters with a `-N` suffix), no object
version ids (the bucket is not versioned), no CRC64 header for objects uploaded without a checksum. The CRC64 verification
(three downloads, then `DOCS_SOURCE_CHECKSUM`) is covered by `crates/oo-connectors/tests/s3_mock.rs`.

The L3 acceptance 5 round on both stores (add, same size modify, rename by copy and delete, delete, cache, secrets):
`../L3/s3-life-rustfs.md` and `../L3/s3-life-minio.md`, 20 of 20 checks each.
