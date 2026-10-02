# Retained author Docker resources — coordinator cleanup

Observed 2026-10-02T18:26:44Z (2026-10-03 Asia/Shanghai). The a098 and a104 author sessions had actually exited; reviewers a110 and a108 use their own resources. This record supplements the earlier REPORT/HANDOFF cleanup observations, which remain unchanged. No product acceptance is granted.

Before removal the coordinator inspected only resource identity, state and labels, never Docker environment or process arguments. a098 container ID `400ea77303d54694da13d8d4d3a0b0ca74d2e3aa3e63d7f938de14790271f196`, name `oo-a098-msrv-e45a72cf-3b73-4708-ad21-5328559ed0b8`, was `exited` with `ExitCode=101`. Its `oo.owner=a098`, `oo.project=open-ontology`, `oo.run_id=e45a72cf-3b73-4708-ad21-5328559ed0b8`, `oo.scope=sp03-lossless-baseline` matched the author's explicit retained baseline. The network had the same labels and zero attached containers. Both a098 volumes had those labels. Both a104 volumes had `oo.owner=a104`, the full session UUID, `oo.project=open-ontology`, and the author's baseline run ID `e44357a9-a8ec-4ce3-a394-22e15537b631`; no container referenced the a104 cargo volume.

The failed a098 baseline log is tracked under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a098/SP03/lossless-types/linux-smoke/baseline/console.log`. Its SHA-256 is `702132e9cd2be4a0105ac8ae8b4d9cb5f32dfd711ee606d54f551ee6343d3d76`. Phase files and the original `101` remain. Removal is not an assertion that the baseline passed.

Exact mutations, in order, each actual command exit `0`:

1. `docker rm 400ea77303d54694da13d8d4d3a0b0ca74d2e3aa3e63d7f938de14790271f196` (no force).
2. `docker network rm oo-a098-net-e45a72cf-3b73-4708-ad21-5328559ed0b8`.
3. `docker volume rm oo-a098-cargo-e45a72cf-3b73-4708-ad21-5328559ed0b8 oo-a098-target-e45a72cf-3b73-4708-ad21-5328559ed0b8 oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104-cargo oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104-target`.

The subsequent exact container/network/volume lookups returned absence diagnostics for all six resources (combined command exit `1`, as expected for absent objects). No prune, image removal, unrelated resource cancellation, or shared-cache removal was performed. The a098 baseline temporary directory/files were not removed; this does not claim all temporary state is gone.

This cleanup does not repair the a104 first failed console/summary overwritten by its author, source SQL zero-write audit, missing scanners, dependencies, or any unsigned product/release gates. No Plan completion box is checked by this record.
