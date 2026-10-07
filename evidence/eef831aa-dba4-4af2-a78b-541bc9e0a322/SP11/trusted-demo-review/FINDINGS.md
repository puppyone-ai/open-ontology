# Findings

Independent review of frozen OPS-D01 `deploy/trusted-demo` at `895864a5bf64e50cef84075d20fd7b0c9e41c264`. The author sessions a305 and a312 wrote the demonstration and the identity delta. This session did not edit the product. The verdict is APPROVE_LIMITED for the combined public-fixture command. It does not accept the parent unit.

## Checked on the fresh a322 run

The operator manifest is required. Bare `start.sh` and `--public-fixture` without `--manifest` exit 2. A missing file, a symlink, a second line, a carriage return, an extra key, a 513-byte file, a wrong archive commit, an uppercase run id, an owner that is not the last four digits, and a run id whose last four digits are not the owner all exit 2. The parser accepts the explicit historical `a305` line and does not start it.

The public fixture and the restart inside that same command both exited 0. Onboarding tests were `passed=24 failed=0 ignored=1` in the pre-overlay smoke and again inside the product builder before workspace binaries. Public phases were 33 zeros. Smoke phases were 7 zeros. North rows were 41, 42, and 43. The restart stdout matched the first north stdout byte for byte. Hidden column and insert both exited 6 with `query-pg: guard`. The policy probe exited 5 with `query-pg: policy`. The bad column `not_a_column` stopped at `reviewed-effective-oos` with `decision.unknown` before column ACL. After the ready receipt, a second `--public-fixture` exited 2 and left the volume timestamps unchanged. `--stop` left the volumes. `--clean` removed only the `a322` containers, volumes, network, and private tmp. A later `--public-fixture` still exited 2 because `live/run` remains. A later `--clean` exited 0.

`schema_hash` `da877f8bc78b7a9a14b8648004bfd904d8bc89b774bbca1253c1758927a74a2f` matches the a305 and a312 receipts. North `original_sha256` `e617988aaadeab6eb1c9302a033a83c2b3ee7df6133fc005b0d1de9079837400` matches the a312 three-row digest. `config_identity_sha256` and both role-pack digests differ from a305 and from a312. The driver puts the manifest's host, database, and reader into the config preimage, and the pack bytes follow that identity. The restart marker `role_pack_unchanged` is the comparison that belongs to this run.

Driver source `60f2ba592e456418e6da94c90e0a4815aa69299bc62adc74046ed07432b9aef1` and artifact `0e0e3a8c90c9d67526c35cd720d7daab30fa69619d04676da4ca2167e23f051c` match the a312 record. `tools/sha256_manifest.rs` stayed `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. Root `Cargo.toml` and `Cargo.lock` were not edited.

Secret scans report `secret_absent`. The evidence tree has no password, key, or certificate file. Builder hostconfig for the preoverlay, the public run, and the restart smoke: memory `6442450944`, NanoCpus `1000000000`, privileged false, cap-drop ALL, OOMKilled false. Mount text has no Docker socket and no exact host-home bind. PostgreSQL publishes no host port.

## Observed and left as the tool wrote it

The ignored onboarding test prints `test live_chain ... ignored, one private PostgreSQL; linux/chain/inside.sh sets OO_A261_*`. That line is in the frozen `25dfc902` test. This review did not edit it. The real PostgreSQL chain is the separate `a322` command recorded under `product/`. The ignored test is not that chain.

The bad-run `outcome.json` remains the copied inspect outcome (`awaiting_review`). The apply result is `apply-outcome.json` (`failed` at `reviewed-effective-oos`).

`query-north-pos.exit` is 6 with `query-pg: guard`. The driver treats that as the positional guard already in the fixture.

`linux/foreign-before.txt` named three other PostgreSQL containers. They were not running after this cleanup. `cleanup/stop.stdout` and `cleanup/clean.stdout` do not name them. The filters require owner `a322`, this run id, and the archive commit. No prune was run.

The product writes its evidence at `evidence/<run_id>/SP11/trusted-demo-identity` relative to the repository that holds the scripts. This review checked out `895864a5` in a private clone so that path was not created in the main worktree. The copy is `product/`. The main tree has no `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a322/SP11/trusted-demo-identity`.

## Not claimed

Authentication, authorization, human approval, semantic publication, natural language, a model provider, a harness, M1, I01, I02, parent acceptance, a release, Compose, and an enterprise connection are outside this verdict. The receipt records the same boundary. A fresh clone supplies its own run id.
