# Report — a176 role-pack closure review

Verdict: **APPROVE LIMITED** the bounded role-pack compiler library slice at `eff1959bf028a65b20b7fca855d79c23e4b188fd`, tree `2f08d4ecef7ad2f3b617e8a04afcb45f0cf0186f`, parent `9e3f757fb34746c64c6fca3d60cf0c5e342a1885`, subject `builder: close role-pack SQL string and name closure`.

The required closures hold on a fresh fixture. Line citations and the kept legal SQL are in `FINDINGS.md`. No product change is required for those findings.

## Scope

Reviewed the immutable git archive of `eff1959b`. The moving worktree was not the source. Slot B’s `Cargo.lock` and `oo-gateway` were not read as the product and were not edited. No product file was edited. No PostgreSQL was started. The oracle is Rust `sha2` 0.10.9 in `overlay/role_pack_a176_review.rs`. It does not call the library hash helper. No Python oracle was written.

Author evidence commit `9f357020` records the repair. That record is the author’s tests. This directory is the independent run. a172 review `95289c50` on `ef34ad7b` is the closure list this commit was written to close. a168 review on `3dadf678` is the F1, F2, and F3 list re-checked here.

## Runs

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Toolchain `1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`, cargo `1.94.0`. Each run: `--cap-drop ALL`, `no-new-privileges`, memory 8g, cpus 4, pids 4096, private network `oo-a176-eef831aa-net`, private volumes `oo-a176-eef831aa-cargo` and `oo-a176-eef831aa-target`, `HOME=/tmp`, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=1g`. `--pull never`. Capacity at each launch was `own=0 running_rust=0 host_cargo=0 global=0`.

| Run | Container | docker exit | What it shows |
|---|---|---|---|
| `linux/smoke/` | `0a28608be85d5fff4ab551589d0f7c13f9b6de8fc538a416e9a890fae8db8fc4` | 0 | Pre-overlay archive. Hash check 10/10. Author `role_pack` 11 passed. Clippy `-D warnings` 0. Offline fetch 101 on the empty cargo volume, then online fetch 0. Overlay absent. |
| `linux/probe/` | `624bf791a3e28a0af6a7565d3f910adab6e19d59cb4839be3c4f5f8d6928f126` | 101 | Harness. Oracle forbade the token `most_common`, which is in the row-security sentence. Author tests 0. Clippy 0. Product digest matches probe-2. Preserved. |
| `linux/probe-2/` | `fb1bfb211c1730cd0977ce1c5000e704b5cd3933ed4c525efff9c693722cd53f` | 0 | Deciding. Product hashes 0. Author tests 0 (11 passed, 0.05s). Oracle 0 (1 passed, 0.11s). Clippy 0. Offline fetch 0 on the warm private volume. |

Probe-2 host config: `Privileged=false`, `CapDrop=["ALL"]`, `CapAdd=null`, `NetworkMode=oo-a176-eef831aa-net`, `Memory=8589934592`, `OOMKilled=false`, empty port bindings, tmpfs size 1g. The launch script removed the container. Overlay content hash on that run is `e51965258d29b0950a033f9e003d5c0fbbebf417989c0913753ed990b3ee865e`. The preserved probe overlay hash is `7161ddcc162212156f95f264744b7a2759cb27311a6cf794a48641d72f9be4fe`.

## Result

SQL `''` is decoded before the hidden-label compare. `'舱''记'` is omitted. `SELECT 'it''s visible' AS label_ok FROM wharf.crate` remains spelled with doubled apostrophes. Prose `it's a visible note` remains.

Unquoted `DISTINCT`, including the degenerate form and the lowercase form, is omitted. `SELECT mass_kg AS "DISTINCT" FROM wharf.crate` remains. Quoted `DISTINCT` is an identifier in this subset.

A relationship and an entity named `vault_bin` are omitted even though their endpoints are visible. The dependent example `rel:vault_bin` is omitted. Legal `crate_scale` and `ex-kept` remain. `declared_fk` is absent.

F1, F2, and F3 hold on the fresh strings: hidden `data_type` and `unit` are blanked, hidden calendar phrases and `quarter_start` are omitted, quoted `"pier"."berth"` is omitted for the clerk, and the NFC and doubled-quote hidden labels are omitted. Legal quoted SQL, non-ASCII prose, the mass metric, the comparison metric, the count metric, and the verified examples remain.

Independent hashes match. Two principals with the same role snapshot share the artifact and differ in the cache key. Refusal, sample, RLS-proof, and manifest behavior held inside the same oracle. `outcome.json` lists no closure, kept, hash, or refusal miss.

The snapshot was not treated as authentication or as a grant. SQL text was parsed and was not executed.

## Still open

Domains, the tokenizer, the read API, search, filesystem mount, authentication, the policy adapter, and publication gates G1–G13 stay open. This review does not approve SP07-W06. No ledger checkbox was marked. No release was cut.
