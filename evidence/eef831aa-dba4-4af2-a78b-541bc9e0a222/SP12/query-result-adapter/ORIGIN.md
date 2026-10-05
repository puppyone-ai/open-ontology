# Recipe origin

Session `eef831aa-dba4-4af2-a78b-541bc9e0a222`. Fixed product commit `c2db9c430b822a824a1062b6c3c7345924df7dd8`. Tree `8fb187147b02d6cacf45cb5a683859f698b497cf`.

The comparator commit `0ae42f942e075be4171646d373c49dcfe1029817` is an ancestor of that archive. Its contract and public types are read from the archive. They are not edited.

| Origin | Commit | Path | SHA-256 |
| --- | --- | --- | --- |
| a204 launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a204 inside | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| a199 manifest hasher | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3` |
| a200 smoke launch | `a587031c719a484830d78e493605f48e2056cae4` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh` | `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` |
| a208 postgres up | `400bfac1` review tree, file as stored | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a208/SP09/query-cli-review/pg/up.sh` | `6c365882159bff7fb30be08056955836208f5855836d8c4214976a280ed15f32` |
| a208 postgres wrap | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a208/SP09/query-cli-review/pg/pg-wrap.sh` | `d707d4e64c060e78245a941719495cf8865783754d953f41d9c503e549cc11d9` |
| a212 smoke launch | `a661dcdf72ff91561ae6b7cb79916e582884d08a` era, file as stored | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a212/SP12/result-comparator/linux/smoke/launch.sh` | `9f7436ddd6b62baa6f5131bc49aae554b4f2df680fc7d966e3e0a34e283b9d00` |
| a212 smoke inside | same | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a212/SP12/result-comparator/linux/smoke/inside.sh` | `1f3d708b6fcab08576e0f06ec62c18daf808e301dd8883130c199c978103efb8` |

`tools/sha256_manifest.rs` is a byte copy of the a199 hasher. No line was edited.

The smoke launcher follows the a212 smoke, which follows the a204 pre-overlay launcher: `git archive` of the fixed commit, a private bridge network, private cargo and target volumes, a capacity gate, `--pull never`, `--cap-drop ALL`, `HOME=/tmp`, offline fetch then one online fetch, phase exit files, and inspect after the container exits. It does not prune and it does not remove another owner's containers.

Changes from the a212 smoke launcher: owner `a222`, fixed commit `c2db9c430b822a824a1062b6c3c7345924df7dd8`, memory 6 GiB, one CPU, global builder cap 10. The smoke target is the archive's `result-comparator` tests. The repository `rust-toolchain.toml` is renamed before cargo runs so the image stays on 1.94.0.

a200 is the private-network and TLS PostgreSQL shape used for the one private database. a208 supplies the PostgreSQL 17.10 entrypoint and the query-pg live fixture.

Proof recipes, after the smoke. `pg/pg-wrap.sh`, `tools/select-debs.rs`, and `tools/sha256.rs` are byte copies. `extract-libclang.sh` and `pg/up.sh` keep the a208 steps and change only the a222 names, the clang root, and the certificate name `oo-a198-pg` that the frozen live test dials. `tools/secret-scan.rs` keeps the a208 scanner and changes the banned fixture literals. `tee.sh` is new: it copies non-empty `query-pg` stdout and still passes that stdout to the test.

| Proof file | SHA-256 | Origin |
| --- | --- | --- |
| `linux/proof/launch.sh` | `e1d6556a8c227ee942f502beb7c1efdfcacd11ed271ee1e81a15608064146463` | a212/a204 launcher, 6 GiB, one CPU |
| `linux/proof/inside.sh` | `c08268fc2bacbec4be1a91c537b9bccacbad09a5b8fec861f6f63ebc04d1535a` | a212 impl-5 lock and fmt gate, then a208 query-pg compile |

Attempt 003 kept the pre-fix inside script `69e2a1398717b9ccb1a1e9c147bd2bcc03a5a20f22b3e87ae8b2307d9f25df99`. After that attempt the script creates `/evidence/libclang` before the extract log redirect (`f67fb33fea30e5c22ffe27c4301e5f67ec5105a5bf7b57d92c50fc2c48fe380c`). Attempt 004 showed Cargo 1.94 writes the `Executable` line to stderr, so the test-binary path is read from both stdout and stderr. Attempt 006 showed a later `--no-run` hardlinks `debug/query-pg` back onto the saved binary inode, so the wrapper is installed with `cp` rather than `mv`.
| `linux/proof/extract-libclang.sh` | `6a39526820f55f7f9508848dc8a08f571c91b2813561601db9e3cede0ef778e0` | a208 `6fda88fc076b63920428fb0c41d74ea4300a25deb27e9e73f9069e32f9be925d` |
| `linux/proof/tee.sh` | `6484d78d7173f098e204714d4d607ffb3cdf731d4695380dad5221c9eb9550ac` | new |
| `pg/up.sh` | `b874e0a9616d5262e8745e9d0aa3dee93ecab26b321cfed53ad1df60146cd0ac` | a208 `6c365882159bff7fb30be08056955836208f5855836d8c4214976a280ed15f32` |
| `pg/pg-wrap.sh` | `d707d4e64c060e78245a941719495cf8865783754d953f41d9c503e549cc11d9` | byte copy |
| `tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` | byte copy |
| `tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` | byte copy |
| `tools/secret-scan.rs` | `3ae3263e5651b6ce8642dac7f437249c4a812af1a35b326371b567f8e888aafa` | a208 `31ecf47c6cd24dc593ba32682afcb880950c3d80a2c16c202cc4065a66452ea7` |
