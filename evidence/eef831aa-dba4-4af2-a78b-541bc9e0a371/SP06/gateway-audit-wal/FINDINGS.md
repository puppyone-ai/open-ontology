# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a371`. Limited to the durability seam that was proved. Not a parent or full-gateway finding.

## Fixed before the passing run

- Full-volume fault token. `fs::write` creates or truncates `last-fault.txt` and then writes. On a 256KiB tmpfs the truncate succeeds and the write of the token needs a new page, so the file stays empty. `DiskWal::open` now reserves one page, and later faults overwrite that page in place. The passing live run recorded `enospc=ok` and `AuditUnavailable` with no query transaction.
- UUID parameters. A `$1::uuid` placeholder is prepared as uuid. `String` cannot serialize that type. Event and receipt SQL cast `($1::text)::uuid` and `($2::text)::uuid`. The passing run delivered two events, deduped them, and kept the same receipt ids.
- Receipt scope. A reused audit database keeps earlier spools. The live assertion reads receipts for the current `spool_id` only.
- Registry subset. Fresh resolution picked patch versions that are not in the root `Cargo.lock` (`cc` 1.6.0, `libc` 0.2.190, `mio` 1.2.4, `hyper` 1.12.0, and the others listed in attempt 7). Both child locks now use root checksums only. `child_locks_are_root_checksum_subsets` passed. Root `Cargo.lock` and root manifests were not changed.

## Left as bounds of this seam

- The feedback receiver is not implemented. This is not a full SP02-W06 claim.
- Public untrusted SQL stays blocked. `UNTRUSTED_ADMISSION` is `Blocked`. The live query is an explicit trusted-operator session with auth false. No user payload builds an authenticated identity.
- `rust-toolchain.toml` remains channel 1.98.1. The proof sets `RUSTUP_TOOLCHAIN=1.94.0` and does not edit that file. A shell that follows the toolchain file will not be the 1.94 proof.
- `deploy/trusted-demo/pg/pg-wrap.sh` still comments an older hash than the file on disk (`05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`). The recipe was not edited.
- `wal-send` prints a short sanitized `down=` line on collector failure. The passing run's secret scan found no password or private-key bytes in the evidence tree. `pg/postgres.log` contains the text of `SELECT rolpassword FROM pg_authid` from the privilege probe that must fail. It does not contain a password value.
- Independent review is still open. E55 parent acceptance is still open.
