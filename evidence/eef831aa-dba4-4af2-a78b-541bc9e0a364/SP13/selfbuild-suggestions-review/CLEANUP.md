# Owned cleanup

One snapshot was taken before removal. It is `handles-before.txt`. This review did not poll and did not wait 18 minutes.

At the snapshot, the only running a364 container was `oo-a364-pg` (image `3aac91d19d5b`, up 11 minutes). No a364 rust builder was running. Global rust builders were 0. Network `oo-a364-sbs-net` contained only `oo-a364-pg`. Volumes present were `oo-a364-gw-target`, `oo-a364-pgdata`, `oo-a364-sbs-cargo`, and `oo-a364-sbs-target`.

Exited containers `oo-a064-rust194-contract2`, `oo-a064-rust194-contract`, `oo-a064-pg1710-jobs`, and `oo-a064-rust194-smoke` were in the snapshot and were not removed. `oo-a361-pg` and `oo-a359-pg` were not in the snapshot. Other named volumes, including every `oo-a355-sbs-*` volume, were left in place.

A password-byte scan of this evidence directory against `admin.pass` and `login.pass` in `/tmp/oo-a364-sbs-secrets` reported `password_hits 0` and `pem_blocks 0` before those files were deleted. The scan output did not include the password bytes.

Removed, by exact name, with no prune:

- `docker rm -f oo-a364-pg`
- `docker volume rm oo-a364-gw-target oo-a364-pgdata oo-a364-sbs-cargo oo-a364-sbs-target`
- `docker network rm oo-a364-sbs-net`
- `rm -rf /tmp/oo-a364-sbs-secrets /tmp/oo-a364-sbs-archive`

Each of those docker commands printed the removed name. Afterward, `docker ps -a --filter name=oo-a364`, an `oo-a364` volume listing, and an `oo-a364` network listing were empty. The temp paths were gone. `oo-a355-sbs-cargo`, `oo-a355-sbs-gw-target`, `oo-a355-sbs-pgdata`, and `oo-a355-sbs-target` were still present. Own rust builders 0, global rust builders 0. That after-state is `handles-after.txt`.

Earlier builders `oo-a364-sbs-smoke`, `oo-a364-sbs-test`, `oo-a364-sbs-querypg`, and `oo-a364-sbs-live` had already been removed by their launch scripts. `smoke-before/container-removed.txt` records the smoke container. The test, query-pg, and live launch scripts remove their named container before they exit.
