# Handoff

Consumers 389, 373, 397, and the frontend can pin these blobs. The commit that adds them is the producer commit. This handoff is not Agent tool-calling and not parent acceptance.

| Path | git blob |
| --- | --- |
| crates/oo-builder/src/role_pack.rs | 51d000f11e0190df38e81141102d8d88c45e90f6 |
| crates/oo-builder/src/reviewed_effective.rs | b30207b6b3a0fec04a41b83663362889c44abaac |
| crates/oo-builder/tests/pack_dialogue_compat.rs | 1a3b6418f9f243c58e5f91ffaf6e054282da1752 |
| tools/ontology-dialogue/src/grammar.rs | 294c620bfd4cfc1d0ad27af234d783d5b71a2c58 |
| tools/ontology-dialogue/src/http.rs | e3e92dc48c18fa090fbac906ab0a6cd3d32ae167 |
| tools/ontology-dialogue/src/propose.rs | 4266388aea3996d0788edef6b3492c487bb449f1 |
| tools/ontology-dialogue/src/main.rs | c9926a3a5550d325d959db256cebf0b1aa4921c7 |
| tools/ontology-dialogue/CONTRACT.md | 01941b3f06ffca4e5cded532c2ff613d7aee6355 |
| tools/ontology-dialogue/tests/identity_compat.rs | 5e0f96c2fdafbbc75151eb4a9a68bfeae6f6cc54 |

Policy id for the live pack: `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e`. Knowledge id: `know_a317`. Pack digest: `755ea5ab90c1a03253c53128b63c1fb4026107fa76719362ff499bf45bbde083`. Proposal sha256: `a0d926f4d953c02bda88cbd4b80197cf20f616bc2554ea87959ef6273c4d2d7d`. Executed cell: int 90000. Citation status: ok.

`contracts/builder/role-pack-v1.md` and `contracts/builder/reviewed-effective-oos-v1.md` still say a policy id is the 63-byte slug. This change does not edit them. Dialogue still rejects a hyphen in a knowledge id. A slug-only policy id remains a valid role pack and is refused as a dialogue config.

The 6-byte bearer is legal only for `development_local` and `http://127.0.0.1`. Production stays HTTPS and 16..=256. Do not reload the a389 100k campaign. The private fixture is `generate_series` of 90000 rows in database `oo_a399`.
