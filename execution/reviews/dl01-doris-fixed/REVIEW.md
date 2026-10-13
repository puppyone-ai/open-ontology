# DL01-W1-30 fixed review

Reviewer session `db7f91a7-f6c3-40f6-a734-502b3e851722`. Author content `d93d595fa41771385c510e704378862ec14e3716`. Seal `5ee729f85a2be6d016d1ddbbbd5ce73c0a3950e4`.

Decision: `approve_setup_dependency`. `request_changes`: none.

Packet: `evidence/db7f91a7-f6c3-40f6-a734-502b3e851722/DL01/doris-fixed-review`. Decisive run `raw/` exit 0 (`failure_count` 0). Preserved reviewer faults: `preserved-attempt-1` and `preserved-attempt-2` (short-string index before any SQL phase) and `preserved-attempt-3` (tuple keys in the column-type JSON, after the oracle checks).

Identity rehashed from the seal commit: 489 of 489 `SHA256SUMS` entries match. The content commit's extra paths are the sums file, `receipt.json`, and the removed `compare.cpython-314.pyc`. FE image `sha256:e86abeb405d5d56ee60da7c3e132f0dddfdbe195206284231e55805c7fc3cf86`, BE image `sha256:21cd6cdf39dfab5c08f21cc695c2d7541325273ee61ec0a8ab108b553e26925e`, both `linux/arm64`. `SHOW FRONTENDS` and `SHOW BACKENDS` report `doris-3.1.4-rc02-7f5ba43de6c`. The MySQL image is a protocol client only. `SELECT VERSION()` returned `5.7.99`. Author `oo-dl01w1-12` resources were absent. The review used prefix `oo-dl01w1-30`, FE 1536 MiB / 0.45 CPU, BE 2304 MiB / 0.45 CPU, and independent fixture values on `192.168.230.0/24`.

Approved scope is the target-preparation setup dependency: separate DBA and read-only reader, duplicate / aggregate / unique key models, DECIMAL(18,4) and DECIMAL(38,4), null versus zero, half-open range partitions, denied reader DML/DDL, stable reader checksum, DBA `KILL QUERY` cancel (`cancel query by user`, 1.314s), and FE audit with no successful reader write.

Outside this approval: Arrow / PostgreSQL / Builder / Pack, Rust driver and MSRV, matrix grade, DBX-W12, full AT-SRC-03 / 07 / 10 / 11, `oo.source.doris/v1`, and any future platform-reader cancel route. `SHOW STREAMS` and `VARBINARY` still fail at parse. `CAST('123456789.01' AS DECIMAL(10,2))` stored `99999999.99`. `PRIMARY KEY` syntax was rejected.

Cleanup removed the review FE, BE, and network. The pinned images remain. Details, raw exits, and hashes are in the packet `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, and `receipt.json`.
