# Dispositions a058

| Item | Disposition |
| --- | --- |
| Inherited-pipe `setsid` hang (a055) | Fixed in `crates/xtask`. Capture files replace reader joins. Host, 1.98.1 arm64, and 1.94.0 arm64 each passed `execute_local_setsid_descendant_returns_bounded_and_spares_a_foreign_process` and the unit setsid test. |
| PATH `kill` no-op hang (a055) | Fixed. `kill` is a validated absolute program. `execute_local_path_kill_is_not_used` passed on the host and on both Linux logs. |
| Output bound | 16 MiB cap. Overflow sets `gate.output_limit` and cannot seal a pass. Unit and CLI output-limit tests passed on the host and on both Linux logs. |
| Foreign process | Same-group and `setsid` tests spare a foreign sleep. The 1.94 live probe left `foreign_after_A=present` after a group signal. |
| Same-group signal vs escaped descendants | A recorded `setsid` descendant is signaled by pid and start token. A survivor is `gate.cleanup` and fails the seal. The code does not treat a group signal as coverage for every process that has left the group. |
| Linux CLI exec failures | Reproduced on the pre-fix snapshot: `/tmp` is `noexec`, script exec exit 126, Rust error 13. Green tests exec from `/target`. The five red CLI failures pass in both green consoles. Infra tmpfs flags were not edited. |
| a057 `evidence_and_gates_self_test` git commit failure | Recorded by a057 on all four recipes. On this red snapshot in the 1.98.1 arm64 image the same test passed, so that failure was not reproduced here. Isolation and stderr-in-the-issue remain. The green self-test passed on the host and on both Linux logs. |
| Imported capture | `execute_local_shim_is_required_satisfied_and_replay_is_not` passed. Replay stays `gate.unverified`. |
| Failed child and source drift | Both CLI tests passed on the host and on both Linux logs. |
| Secret gate | Pending. Stays red. |
| W05 and CI | Pending. Unimplemented. |
| Release authority | Not granted. |
| Full four-recipe matrix | Not run. Focused proof is arm64 1.98.1 and arm64 1.94.0, plus the host. |
| amd64 | Not run in this session. |
| Airgap | Not claimed. Toolchain component downloads used the network before `--offline` cargo. |
