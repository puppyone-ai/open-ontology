# Cleanup

Removed only Docker objects labeled `oo.owner=a167` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a167`, plus this session's `/tmp/oo-a167-*` files. No prune. No foreign container, network, volume, or image was removed. The pinned Rust image stayed local.

Observed at `2026-10-04T19:13:07Z`. Every removal exit was 0. Afterward the same label filters listed no containers, networks, or volumes.

```text
=== before containers ===
oo-a167-msrv-a167test03 Exited (0) a167test03
oo-a167-msrv-a167test02 Exited (101) a167test02
oo-a167-msrv-a167test01 Exited (101) a167test01
oo-a167-msrv-a167preedit01 Exited (0) a167preedit01
=== before networks ===
oo-a167-a167preedit01-net
oo-a167-a167test01-net
oo-a167-a167test02-net
oo-a167-a167test03-net
=== before volumes ===
oo-a167-a167preedit01-cargo
oo-a167-a167preedit01-target
=== rm containers ===
oo-a167-msrv-a167test03
oo-a167-msrv-a167test02
oo-a167-msrv-a167test01
oo-a167-msrv-a167preedit01
container_rm_exit=0
=== rm networks ===
oo-a167-a167test03-net
oo-a167-a167test02-net
oo-a167-a167test01-net
oo-a167-a167preedit01-net
network_rm_exit=0
=== rm volumes ===
oo-a167-a167preedit01-cargo
oo-a167-a167preedit01-target
volume_rm_exit=0
=== after containers ===
=== after networks ===
=== after volumes ===
tmp_rm_exit=0
```
