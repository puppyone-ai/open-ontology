# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a301`. Owned paths: `tools/cdc-apply-pg/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a301/**`.

This handoff is not parent acceptance. ACK, XACK, TRIM, and a control-database mirror are not implemented. a299 spool approval is still required. CDC-D04 and full SP04 are not accepted. Source and WAL cross-table certification stays false.

## Delivered

Standalone Rust workspace `tools/cdc-apply-pg`. CLI `cdc-apply-pg apply <request.json>`. Password and CA are file paths. One dedicated target connection performs the fence adopt, the fence check, the `commit_apply_receipt` call, and the selected row writes in one SQL transaction. Replay of the same payload does not write again. A changed payload for the same batch identity returns `OO010`. Unknown commit I/O reconciles the durable receipt on a new connection before returning applied. If that receipt cannot be proved, the result is `commit_unknown` and nothing is acknowledged.

The final locked offline proof is `linux/proof`: test exit 0, Clippy exit 0, docker-run exit 0, lock subset 284 checksums, lock SHA-256 `67d47b32d081d28504a3d0fe0fcb431c49c618dd1f5599fa188d6f8878e1e78f`. Earlier attempts remain under `linux/proof-1` through `linux/proof-7` and `linux/precode`.

`tools/sha256_manifest.rs` is a byte copy of the a199 tool, SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. `SOURCE.sha256` lists `tools/cdc-apply-pg`. `MANIFEST.sha256` lists this evidence directory and skips itself. Both are produced by that Safe Rust program inside the pinned 1.94 Linux arm64 image. Empty SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`. `abc` SHA-256 `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

## Not delivered

- Parent or loader acceptance.
- Segmented apply above 200000 events.
- A real Debezium certificate or an upstream LSN proof.
- Edits to migrations, root crates, root lockfiles, or connector manifests.
- A generation-count assertion after payload conflict and row failure. See FINDINGS item 1. The receipt rollback is what the live test checks.

## Runtime contract the next caller must keep

Pass a recovered committed spool, a trusted TLS endpoint whose host equals `server_name`, the mapping and type-policy binding, and the current fence owner. Declared cell types are only `bool`, `int2`, `int4`, `int8`, and `text`. Unavailable on update leaves the stored value. Unavailable on a required insert refuses. A stable non-null primary key is required. Do not treat exit 0 of a launcher as acceptance; read `linux/proof/test.exit` and `linux/proof/clippy.exit`.
