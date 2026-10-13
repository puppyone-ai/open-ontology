# DL01-W1-77 handoff

接收人：原 06 工程。一份返工，同一所有者。不新开功能分母。本目录不修改 Loader、迁移、root Cargo、dispatch 或 UPDATE-PROTOCOL。

判定：`REQUEST_CHANGES`。全源验收、S5、Gateway、production、published 角色、客户夜跑保持 pending。完整签收 0/10 与预览 6/10 不因本复审改动。

## 要改的行为

1. `crates/oo-pipeline/src/native_update.rs` 的 `bind_admission`（524–598）与单测 `producer_receipt_binds_the_document_not_the_expect`（2776–2860）。自写 JSON、文件与文档自相等、文件自身 sha256、以及与主文档不一致的 `admission_expect`，都不得得到 `trusted true`。核验应走到已有的 `native_binding.rs` `verify_inputs`（690–751）或等价的服务端工件打开：catalog、registry、driver、Arrow、audit、run 身份。
2. 保留 `declared_is_self`（674–684）对 digest 常量的 `digest_mismatch`。文件自身 sha256 不再是信任令牌。见 `check_admission_receipt` 506–507。
3. `classify` 319–323 与水位写入 `0010_native_update_expand.sql` 475–495。调用方 grade T 与 `cross_table_consistent true` 不得落成观测等级。
4. 删除 1489–1490 行对 `applied_pos_request` 与 `audit_digest` 的丢弃。请求 `"0"` 在有父代的更新上必须拒绝。不得写入 `_oo_pos` 1000000 起的行，不得把水位写成 1000002，不得把 `position_state` 写成 504 行的字面量 `ok`。约束 84 行已允许 `expired`。
5. `payload_digest`（1880–1916）或 replay 比较（171–181）要覆盖 truth、grade、位点、audit。digest 内的墓碑变化继续 `OO010` 且无批次。重叠键的 `conflict` 继续无批次。
6. `jobs_lease_interface`（1720–1731）与 `crates/oo-jobs/src/lib.rs`。要么实现真实 claim 与旧 owner fence，要么租约缺失时拒绝封存。`target_fence_certified` 在 receipt 为 null 时保持 false，且封存不得发生。

复现入口：`raw/harness/probe.py`。决定性用例是 `forged-file-grade-t`、`expired-pos-child`、`replay-same`、`replay-oo010`。对照用例 `cross-source`、`self-equal-hash`、`bad-type-utf8`、`replay-payload-tombstone` 应保持拒绝且无批次。

## 不要混进通过

Oracle int2/utf8 物理装载仍是作者记录的 `bad_type`。Mongo `public.docs` 的 `cross_source` 是目录拒绝，不是 16 行产品导入。pg-builder `ready false` 与可调用语义页分开。无 admission 的 fixture 批次保持 `source_bound false`，不与 source-specific 全单元格证明混用。无 `admission_path` 的成功装载不是 trusted capture。

`oo-core` 的两个字典测试与 `pg-builder` 的五个 source_contract 测试，在 catalog 树在归档内时退出 0。它们不代替 Loader 信任门。

## 可用工件

| 工件 | 值 |
|---|---|
| 被审提交 | `f0060dfbc217ec3cf1376dc7e873878acee88081` |
| 作者证据 | `evidence/b5ac4bc8-1872-4fe4-a56a-04a70f7103a2/DL01/native-physical-import/source-bound-loader` |
| 作者清单 | 207 条，本复审重算 0 不符，见 `source-inventory.txt` |
| 安装 digest | `74a2db86560e6354142a447d886d22fb7bc19b6b53cf027439028b5aadd1d617` |
| 本复审 ELF | `binary-hashes.txt`。debug，未入库 |
| 探针原文 | `raw/probes/`、`raw/phases/`、`raw/sql/confirm.sql.out` |
| 依赖 blob | `dependency-inventory.txt` |

CLI 入口在 `crates/oo-pipeline/src/bin/oo-pipeline.rs:32–37`：`import-native-update` 与 `install-native-update`。多余参数打印 `oo-pipeline: usage` 并退出 2。`ssl disable` 只接受 host `127.0.0.1`。

## 资源

前缀 `oo-dl01w1-77`。目标容器 `583f6f696f30c760fa33d6ee5c77281b1157c6b94356c506fed5f8748f957c0c`，库 `oo_dl01w1_77`。控制容器 `202fb68dbc1ceb9cc68806f8e07b1a12c07feab03ce5e211478e7172731cea86`，库 `oo_dl01w1_77_control`。镜像 Postgres digest 见上。两库无发布端口，网络 `bridge`。自定义网络 `oo-dl01w1-77-net` 因地址池耗尽未创建，没有执行 network prune。NanoCpus、Memory、CpuQuota 均为 0，即无配额。2026-10-12T01:13:27Z 观测：目标 0.00% CPU、30.68MiB；控制 0.66% CPU、26.61MiB；宿主 16 CPU，Docker 可见内存 94.12GiB。编译容器当时已退出，本记录没有编译峰值采样。`raw/cleanup/resource-observe.txt` 只有 `nproc=16` 与 `jobs=unlimited`。

2026-10-12T01:16:42Z 已执行 `docker rm -f -v`，仅删除 `oo-dl01w1-77-target`、`oo-dl01w1-77-control`、`oo-dl01w1-77-trust`。过滤名 `oo-dl01w1-77` 之后为空。密码文件已删。`oo-dl01w1-06-pg`、`oo-dl01w1-06-mysql`、`oo-dl01w1-06-cellpg` 仍在。没有 volume prune，没有 network prune。宿主暂存 `/tmp/oo-dl01w1-77` 保留 archive 与 ELF，不入库。详见 `raw/cleanup/cleanup.txt`。

## 下一步

06 按上面六条改同一 Loader，并用同一类探针证明：伪造 receipt、过期位点、truth 变异均无批次；`cross_source`、digest 常量、`bad_type`、`OO010`、重叠墓碑冲突仍无批次；租约要么真实持有要么拒绝封存。通过后再开新的固定复审。本文件不关闭 DBX-W05，不关闭全源。
