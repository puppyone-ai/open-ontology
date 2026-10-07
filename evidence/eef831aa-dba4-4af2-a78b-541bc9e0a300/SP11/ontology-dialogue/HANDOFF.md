# Handoff

The trusted-operator command for session `eef831aa-dba4-4af2-a78b-541bc9e0a300` is in `tools/ontology-dialogue`. Proof-8 on fixed archive `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65` recorded `live_done=1`. Read `REPORT.md` for the oracle and the failed attempts. Read `FINDINGS.md` for the 256 MiB binary cap, the count result, and the citation limits. Source hashes are `source.sha256`. The proof inventory is `linux/proof-8/MANIFEST.sha256`.

Keep proofs `linux/proof` through `linux/proof-7`. Their docker exits are 101. Smoke `linux/smoke/launch.exit` is 0 and was not rerun.

The dialogue `Cargo.lock` stays the subset that `cargo fetch --locked` already accepted. Do not run `cargo update` or `cargo generate-lockfile` for this crate. Do not edit the root lock from this unit. a313 remains the root dependency writer.

`ontology-dialogue propose` and `ontology-dialogue execute` are the operator commands. Execute hashes `query-pg` after the approval check and then runs that binary with `--request`. The password file is read by the child. The approval file is a declaration. The audit receipt does not authenticate a session principal, and this command does not judge entailment.

Still parent work: hosted Gateway authentication, untrusted parser admission, `runsc`, and the NL harness. Do not treat proof-8 exit 0 as acceptance of those.

Cleanup after this packet removes only the a300 resources: container `oo-a300-pg`, network `oo-a300-eef831aa-net`, volumes `oo-a300-eef831aa-cargo`, `oo-a300-eef831aa-target`, and `oo-a300-eef831aa-pgdata`, and the directory `/tmp/oo-a300-eef831aa-dba4-4af2-a78b-541bc9e0a300`. The result is `CLEANUP.txt`.
