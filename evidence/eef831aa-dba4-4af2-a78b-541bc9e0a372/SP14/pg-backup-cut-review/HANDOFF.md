# Handoff

```
session=eef831aa-dba4-4af2-a78b-541bc9e0a372
task=a372
role=independent-original-e51-review
fixed_product=20f065096fa9e20414eed8951d830333160e4af6
head_at_report=e090131ed9df0f3b4d39f126ddfc389c2d129722
product_tree_matches_fixed=true
product_edited=false
owned=evidence/eef831aa-dba4-4af2-a78b-541bc9e0a372/SP14/pg-backup-cut-review/**
verdict=WITHHOLD
missing_clause=cut_proven and replay_ready certify fixture views oo_dict.calendar and oo_dict.enums plus schema oo_a372_probe table added sequence seq and view v created during the held SHARE window
e51=WITHHOLD
parent_acceptance=false
w04_gate=open
pitr_ready=false
all_state_certified=false
production_ops=false
production_slo_certified=false
offset_min_used=false
shared_snapshot_claimed=false
general_pg_support=false
exact_fixture_only_rejection=false
enum_calendar_owner=a365
enum_calendar_edited=false
oracle_101_rerun=0
gold_rs_blob=2564210b226be3d6980265a1dc429cfadf2a613f
shop_party_sha256=62230764135b6d717fe792dffa77d776d0e7cdc7ff2123fd049ce4f56f06fa5b
shop_account_sha256=fef89f25c29e9ed5b9c36d0bf37f3056433c33ade60b90978d4f91e7c0c7a958
shop_entry_sha256=a11369572ab597c2211d47df03b92c2a1b2da88451bfb5439c1d7ecf2cb10450
agent_dump_sha256=43935e2e18d631e1a044d190a5b7e84f2f3045b6a0bc53646b38f45b63e4031d
control_dump_sha256=801b515442bc2575e9ae059b3f9850f35127a47b3ae7910c85a66ba467dbfbb5
rustc=1.94.0 (4a4ef493e 2026-03-02)
platform=linux/arm64
rust_image=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
pg_image=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
pg_version=(PostgreSQL) 17.10
cargo_update=no
root_manifest_changed=no

pre_edit_smoke_inside=0
product_test=0
product_clippy=0
product_build=0
check_test=0
check_clippy=0
check_build=0

load_fixture=0
backup=0
locks_seen=1
rejected_update=3
rejected_sqlstate=55P03
amount_during_lock=1234567890.1234
committed_update=0
source_amount_after=1.0001
restore_amount=1234567890.1234
marker=cut-a356-1|100000
entry_count=90000
mark_post_cut=0
restore_corrupt_dump=4
corrupt_replay_ready=false
corrupt_offset_published=null
dbs_before_corrupt=none
dbs_after_corrupt=none
restore=0
restore_existing=4
cut_check=0
run_live=0
cut_proven=true
replay_ready=true
history_insufficient=false
offset_published=100000
wal_moved=false
agent_view_share=0
control_view_share=0
agent_lock_after_ddl=1
control_lock_after_ddl=1
boundary_in_restore=true
boundary_comment=a372-during-cut
boundary_added=1
boundary_sequence_last_value=1
boundary_view_n=7

password_leak_files=none
private_key_files=none
new_unit=false
```

The hash tree is written by `pg-backup-restore hash-tree` after `cleanup-done.txt`. Host `shasum` is not the tree authority.
