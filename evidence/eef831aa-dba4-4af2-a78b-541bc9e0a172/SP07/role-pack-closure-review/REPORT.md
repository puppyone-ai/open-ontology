# Report — a172 role-pack closure review

Verdict: **REQUEST_CHANGES** on the library slice at `ef34ad7bc6d89376d0b1952dfe49e5efb2a8744d`, tree `9f2f6d353a957cc400c7be01d575599260117fe4`, parent `06a99f1181b648863975eb214c14f949d835a67e`.

One closure hole remains. A hidden column label `签'记` is copied into clerk `metrics.md` as the supported SQL string `'签''记'`. The metric heading is `## sq_leak`. Line citations and the kept legal SQL are in `FINDINGS.md`.

## Scope

Reviewed the immutable git archive of `ef34ad7b`. The moving worktree was not the source. Slot B's `Cargo.lock` and `oo-gateway` were not read as the product. No product file was edited. No PostgreSQL was started. The oracle is Rust `sha2` 0.10.9 in `overlay/role_pack_a172_review.rs`. It does not call the library hash helper. No Python oracle was written.

Author evidence commit `89cc545d690f36794d6a6b4bb3356f5ea05a56a7` records that repair. The product subject is `builder: close role-pack visibility for quoted names and labels`. That record is the author's tests. This directory is the independent run.

a168 review `d61f9a4c` on `3dadf678` is the defect list this commit was written to close: hidden names in `data_type`, `unit`, and calendar phrases; a quoted `schema.table` of a hidden dataset; a non-ASCII hidden label in verified SQL, metrics, glossary, and calendar phrases.

## Runs

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Toolchain `1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`. Each run: `--cap-drop ALL`, `no-new-privileges`, memory 8g, cpus 4, pids 4096, private network `oo-a172-eef831aa-net`, private volumes `oo-a172-eef831aa-cargo` and `oo-a172-eef831aa-target`, `HOME=/tmp`, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=1g`. `--pull never`. Capacity at each launch was `own=0 running_rust=0 host_cargo=0 global=0`.

| Run | Container | docker exit | What it shows |
|---|---|---|---|
| `linux/smoke/` | `c346a961e589aff4b376fb0a7cba3ffed3986958560d8d8b62b2cf5660a1c01e` | 0 | Pre-overlay archive. Hash check 0. Author `role_pack` 10 passed. Clippy `-D warnings` 0. Offline fetch 101 on the empty cargo volume, then online fetch 0. |
| `linux/probe/` | `3ce02177ab21adbd7c73dbafcb8b95e36431ba61145f597cf88833cb53c6e108` | 101 | Harness. Enum value status `unexplained` with class `open` failed OOS at `/values/1/class` before any closure assert. Clippy component was absent in that container. Preserved. |
| `linux/probe-2/` | `9ea7e389ee1085450088e2fcfc48f4b72ae7f31bb7d8343523c71ac971c7ed74` | 101 | Same product leak as probe-3. Clippy `collapsible_if` in the oracle. Preserved. |
| `linux/probe-3/` | `0d8e4ef3ac35ede11c8b05b029c8a0af3bdebefd16f14841d903dd71ded8760b` | 101 | Deciding. Product hashes 0. Author tests 0 (10 passed). Oracle 101 with the two `sq_leak` violations. Clippy 0. |

Probe-3 host config: `Privileged=false`, `CapDrop=["ALL"]`, `CapAdd=null`, `NetworkMode=oo-a172-eef831aa-net`, `Memory=8589934592`, `OOMKilled=false`, empty port bindings. The launch script removed the container.

## Result

F1 is closed on the fresh fixture. `data_type` `numeric hold_bin` and unit `HOLD_BIN` do not appear. The `mass_kg` row remains with an empty type cell. Calendar phrase `本季` remains. Hidden anchor `quarter_start` and the phrase that uses it are omitted.

F2 is closed. `"pier"."berth"`, `"Pier"."Berth"`, and `"pier" . "berth"` drop their metrics.

F3 is closed for an NFC label, an NFD label, a calendar phrase label, and a quoted identifier whose `""` decodes to the hidden label `签"记`. F3 is open for the SQL string form of `签'记`.

Legal quoted SQL, non-ASCII prose, the mass metric, the comparison metric, the count metric, the apostrophe string `it''s visible`, and three verified examples remain. The compiler did not omit the whole pack.

Independent hashes match. Two principals with the same role snapshot share the artifact and differ in the cache key. Refusal, sample, RLS-proof, and manifest behavior held inside the same oracle. The violation list contains only the string-label leak.

`SELECT DISTINCT FROM dock.parcel` is accepted because `DISTINCT` is not a keyword. `SELECT DISTINCT mass_kg ...` is omitted. A relationship and an entity whose own name is the hidden column `hold_bin` are copied in a side compile. Both are notes in `FINDINGS.md`.

The snapshot was not treated as authentication or as a grant. SQL text was parsed and was not executed.

## Still open

Domains, the tokenizer, the read API, search, filesystem mount, authentication, the policy adapter, and publication gates G1–G13 stay open. This review does not approve SP07-W06. No ledger checkbox was marked. No release was cut.
