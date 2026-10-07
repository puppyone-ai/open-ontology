# Handoff

```
session=eef831aa-dba4-4af2-a78b-541bc9e0a368
task=a368
product_base=1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9
owned=tools/pg-backup-restore/**,evidence/eef831aa-dba4-4af2-a78b-541bc9e0a368/SP14/pg-backup-quiescent-cut/**
e51=unaccepted
parent_acceptance=false
w04_gate=open
pitr_ready=false
production_slo_certified=false
enum_calendar_owner=a365
enum_calendar_edited=false
oracle_101_rerun=false
cut_mode=writers_blocked
caller_quiescent_certificate=false
rustc=1.94.0 (4a4ef493e 2026-03-02)
platform=linux/arm64
rust_image=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
pg_image=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
cargo_update=no
root_manifest_changed=no

pre_edit_smoke_inside=0
pre_edit_smoke_docker=0
product_test=0
product_clippy=0
check_test=0
check_clippy=0
product_build=0
check_build=0
builder_docker=0

load_fixture=0
backup=0
rejected_update=3
rejected_sqlstate=55P03
committed_update=0
mark_post_cut=0
restore_corrupt_dump=4
restore=0
restore_existing=4
cut_check=0
run_live=0

pg_wrap_sha256=d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb
shop_entry_gold_sha256=a11369572ab597c2211d47df03b92c2a1b2da88451bfb5439c1d7ecf2cb10450
amount_during_lock=1234567890.1234
amount_source_after=1.0001
amount_restore=1234567890.1234
marker=cut-a356-1|100000
entry_count=90000
restore_databases_before_corrupt=none
```

Raw phase exits are in `live/raw-exits.txt`. Failed attempts are in `failed-attempts/`. The evidence hash tree is `SHA256SUMS`, produced by `pg-backup-restore hash-tree` inside the pinned Rust image after cleanup. Host `shasum` is not that tree.

Cleanup removes only containers, volumes, and the network whose `oo.owner` is `a368` and whose `oo.session` is this UUID, plus `/tmp/oo-a368-eef831aa-dba4-4af2-a78b-541bc9e0a368`. It does not prune.

The next session does not inherit an accepted E51. The parent goal and W04 stay open.
