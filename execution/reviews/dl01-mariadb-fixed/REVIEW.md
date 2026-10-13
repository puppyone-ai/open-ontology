# DL01-W1-29 fixed review

Reviewer session `048eb598-9e8b-4442-b9f4-33929685bdb5`. Author content `ed892b5eba9a055dd36fbfc5865323afbdbd0c83`. Seal `eb031c82c36d5b625feeb0825f6587c8dffc384d`.

Decision: `approve_setup_dependency`. `request_changes`: none.

Packet: `evidence/048eb598-9e8b-4442-b9f4-33929685bdb5/DL01/mariadb-fixed-review`. Decisive run `raw/attempt-03` exit 0 at `2026-10-09T16:50:27Z`. 76 checks passed. Preserved `raw/attempt-01` exit 1: the review client split `--defaults-extra-file` into two arguments, MariaDB 11.4.13 rejected the option, and the ready server was never queried. Preserved `raw/attempt-02` finished the native checks with four reviewer assertions wrong; the corrected assertions are the attempt-03 result.

Identity rehashed from the seal commit, not the worktree: 161 of 161 `SHA256SUMS` entries match, sums digest `2c3609590d04448db80f7b92b7dc356e5cc969dd32ae8324dc41715cdc8f0394`. Workspace drift against the content commit is 0. Base `577160c22de63adb9b38e0147f3bc2da893c4d9d` has no `testing/dl01-targets/mariadb` tree. Image `sha256:0130d92c05fbf2d82adc2b86de742eaede65b2596c65d91786f8e03cd19e6a39`, native `aarch64`, `VERSION()` `11.4.13-MariaDB-ubu2404`, comment `mariadb.org binary distribution`. Client `mariadb` 11.4.13, client 15.2. Author `oo-dl01w1-08` resources were absent. The review container used prefix `oo-dl01w1-29`, 1536 MiB, 1 CPU, schema `oo_w129`, and accounts `w129_dba` / `w129_reader`.

Approved scope is the target-preparation setup dependency: separate DBA and read-only reader, exact unsigned and decimal text, NULL versus empty versus zero, JSON as `longtext` with `json_valid`, temporal values, distinct `VARBINARY` keys, `utf8mb4_bin` versus server collation `utf8mb4_uca1400_ai_ci`, statement audit with `write=0`, reader denials including `KILL QUERY` error 1095, DBA `KILL QUERY` cancel error 1317, and cleanup.

Outside this approval: Arrow / PostgreSQL / Builder / Pack, driver compile and MSRV, matrix grade, DBX-W06 as a whole, full AT-SRC-03 / 07 / 10 / 11, CDC, and `oo.source.mariadb/v1` as an implementation. `log_bin` is 0 and `gtid_current_pos` is empty. The version string is MariaDB 11.4.13. It is not a MySQL result.

Cleanup removed the review container, volume, network, and secrets. The pinned image remains. Raw exits, hashes, and the receipt are in the packet.
