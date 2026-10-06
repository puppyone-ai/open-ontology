# Handoff

The producer sort is commit `56b791368d7b5ec050aa36ac91d33bc2234bc4d2` on top of workspace `333406912ddeee60af04b7026d6231a465cd3dc2`. The proof archive was fixed `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a`, tree `525a432045d17087ee412be0d35550e94c55142a`. Before that commit, `description_candidate.rs` at the workspace HEAD matched the fixed blob.

`selected_datasets` is the sorted dataset strings on every outcome. Cards stay in `(schema, name)` order. Candidates stay in dataset order. The contract file was not rewritten.

Evidence is only under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a262/SP07/description-selection-order/`. `linux/smoke` is the pre-edit MSRV smoke. `linux/red` is the preserved failing public-API run. `linux/proof` is the patched library, focused tests, and Clippy. `linux/cli` is the unchanged description-candidate binary consuming that library. Own containers, volumes, the private network, and `/tmp/oo-a262-eef831aa-dba4-4af2-a78b-541bc9e0a262` are gone.

An independent review of `56b79136` is the next step for frozen S5-D01. This handoff does not accept the parent goal, does not mark S5-D01 done, and does not open a column, transport, residency, cost, or CLI unit.
