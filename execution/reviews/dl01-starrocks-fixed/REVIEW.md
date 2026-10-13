# DL01-W1-23 fixed review

Reviewer session `fa610af9-30e5-44f9-a49b-d35a49447dc8`. Repaired author content `970311b5872a4957beaacc19c1280827dda2a87b`. Seal `d56631360d6f7852ff079431aaba735ad2416f0f`. Prior request_changes `8fb8fb334eb40f1261c4ff9cf702a8fed79cdc4d`.

Decision: `approve_setup_dependency`. `request_changes`: none.

Approved scope is the target-preparation setup dependency at `--cpus 1` and 4GiB. A primed `w123_reader` session (`16777252`) issued `KILL QUERY 16777253` while one business connection was visible. The business client exited 1 with `ERROR 1064` `killed manually` in 3.128 seconds. Counts were business 1 and control 1, both `w123_reader`. The setup DBA was not used. The reader grant is `SELECT` only, with no `OPERATE`. Memory stayed `4294967296` and NanoCpus `1000000000`. One BE reported CpuCores 1 and MemLimit 3.240GB.

Packet: `evidence/fa610af9-30e5-44f9-a49b-d35a49447dc8/DL01/starrocks-fixed-review`. Identity: 289 of 289 seal digests match the content commit. Seal parent is `fd909f00d8007829cc0f28bbfa75a5d108100da7`; content is an ancestor of the seal. Image `sha256:9b98696ee0cb46831eabe197ae5e71f483ee3c2e16ab57fd27cb51a5c9ee6618`, `linux/arm64`, `current_version()` `3.5.21-85fccf8`. Journey exit 0 at `2026-10-09T17:44:33Z`.

The prior 90.316 second `ERROR 5024` failure is preserved at `attempts/20261009T154622Z`. The harness abort is preserved at `attempts/20261009T153950Z`.

Outside this approval: Arrow / PostgreSQL / Builder / Pack, driver compile and MSRV, matrix grade, DBX-W12, and full AT-SRC-03 / 07 / 10 / 11. The MySQL wire string `8.0.33` is not a MySQL or Doris result.

Cleanup removed the review container, network, and secrets. The pinned image remains.
