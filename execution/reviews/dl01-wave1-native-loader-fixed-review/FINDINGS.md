# DL01-W1-77 findings

固定文件均来自 `f0060dfbc217ec3cf1376dc7e873878acee88081`。`native_update.rs` 与 `native_binding.rs` 的 git blob 与 `2e0bd098363ace48bda9b9127f5fabe6d3a1647d` 相同。`source_bound.rs` 与 `8397fd5b984df364cc3638ffa1e69ea06e125d74` 相同。`jsonb_doc.rs` 与 `983577c9b89453b08ab4e1a02cab88df78c3e005` 相同。行号按该 blob。

## F1 自写 receipt 被当成获准 producer

`trusted_capture`（`crates/oo-pipeline/src/native_update.rs:506`）调用 `bind_admission`（同文件 524）。有 `admission_path` 时读取调用方给出的文件。历史哈希走 `source_bound false`。其余路径调用 `bind_document_to_producer`（641），只比较文档与该文件的 engine、source、generation、snapshot，以及 schema、type、config、input、audit digest。

随后 558–575 行构造 `CaptureReceipt`。`catalog_sha256`、`registry_sha256`、`driver_digest`、`arrow_sha256`、`arrow_schema_sha256` 取自刚刚解析的同一文件。`check_admission_receipt`（`native_binding.rs:483`）再拿文件和这个结构比。536–544 行的 catalog、registry、driver 比较因此是文件对自身。506–507 行要求 `declared_receipt_sha256` 等于文件 sha256。`declared_is_self`（`native_update.rs:674`）在声明值等于文件 sha256 时返回 false（678–679），所以自身哈希不会被这条函数挡住。声明值若等于 schema、type、config、input 或 audit digest，则返回 `digest_mismatch`。

`verify_inputs`（`native_binding.rs:690`）会打开 report、config、catalog、receipt，并核对 driver 的 kind、name、version、digest 与 catalog 身份。调用点在旧绑定路径。`trusted_capture` 不调用它。

单测 `producer_receipt_binds_the_document_not_the_expect`（`native_update.rs:2776`）在进程内写下文档和 receipt，把 `schema_digest` 复制进 catalog、registry、driver（2809–2811），把 `admission_expect` 设为 doris / `oo_u06_fix` / generation 9（2821–2826），然后断言 `source_bound true`（2828、2835）。这个断言把错误契约写成了期望。`pipeline-trust-final` 退出 0，日志是 `running 0 tests`、`54 filtered out`。该单测这次没有执行。实库探针执行了同一构造。

探针 `forged-file-grade-t`：

- receipt 由探针写出，schema `oo.source-admission.capture-receipt/v1`，engine clickhouse，source `oo_u06_bind`，generation 1。
- catalog sha256 为 `cd`×32。registry、driver、各 digest、audit 为 `ab`×32。
- Arrow 使用作者 `gen1.arrow`，文件 sha256 `ea5c4ded359e549417da7af4e6ec651fb58109c3bfb857b5367c3b10bf5218a7`，schema sha256 `f4ebfa2fc6765828fee6e8db1d021002c39d0e1896678c12248b04bb838d0439`。
- `declared_receipt_sha256` 等于该 receipt 文件 sha256 `3e307db188c07c6c5e45d07e5583ca56ae8ba62b6b3a7e0af4fe2a78c09e4a6a`。
- `admission_expect` 为 doris / `oo_u06_fix` / generation 9。
- `consistency_grade` 为 T，`cross_table_consistent` 为 true，`applied_pos` 为 `"0"`。

退出 0。`source_bound true`，`producer_trust.trusted true`，`document_bound true`，`self_equal_digest false`，`entry` 为 `capture_receipt_file`。`load_state awaiting_publish`，`row_count 3`，物理表 `pt_891c121f65e6a43fd18f4641ab0a5ee30dc75fcb53be9ffe87628439cf42`，payload digest `5869b2bf86c1b2e572c8e85d80fe35bf023dd00dfc5e53b75d030040769543a5`。单元格 `_oo_pos` 0、1、2，业务列 101/10/a、102/20/b、103/30/c。

返工：`trusted_capture` 必须对照服务端已获准的 producer 工件核验 receipt。现成的 `verify_inputs` 或等价路径要打开真实 catalog、registry、driver、Arrow、audit 字节，并使用该 producer 的 run 身份。文档与 receipt 的 engine、source、generation、snapshot、catalog、type、config、input、Arrow、driver、audit 必须对着这些工件，而不是对着调用方刚写的同一 JSON。`admission_expect` 与主文档不一致时必须拒绝，且 `_oo.update_batch` 无该行。单测 2821–2835 的期望要改成拒绝。

复现：archive `f0060df`，用本次 ELF 或同提交重建的 `oo-pipeline`，安装 0010，按 `raw/harness/probe.py` 的 `forged-file-grade-t` 提交。修复后期望退出非 0，reason 为拒绝，`w77-forged` 行数为 0。

## F2 声明哈希等于 digest 常量时已拒绝

探针 `self-equal-hash` 使用同一份 forged receipt，把 `declared_receipt_sha256` 设为 `ab`×32。退出 1，reason `digest_mismatch`，批次 0。这条要保留。文件自身 sha256 不能继续充当信任令牌。

## F3 调用方 grade T 被落成观测等级

`classify`（`native_update.rs:319–323`）把 `consistency_grade` 收成闭合记号 T、S1、S0、B。grade 不是 T 且 `cross_table_consistent` 为 true 时返回 `consistency`。grade 为 T 且该布尔为 true 时通过，不读字典。封存把 `plan.consistency_grade` 绑进 SQL（1453 行）。`0010_native_update_expand.sql:475–495` 把它 upsert 进 `_oo.source_watermark`。

探针之后水位是 `oo_u06_bind | T | true | 1000002 | lagging`，source generation 2，target generation 2。

`tools/pg-builder/src/source_contract.rs:132–133` 把角色包 `ready` 与 `partial_ready` 固定为 false。重跑的 5 个 `source_contract` 测试通过，其中包括 `derived_source_bound_projects_native_notes_without_a_forged_grade`。这条只覆盖 Builder 包路径。Loader 仍接受调用方 T。

`crates/oo-core/src/source_bound.rs:379–391` 的 `optional_grade` 接受调用方字符串 T、S1、S0、B。字典推导测试在 catalog 就位后 2 passed：`real_oracle_dictionaries_derive_three_flags_without_stamped_grade` 与 `cross_source_digest_column_and_token_are_refused`。`unconstrained_number` 在 `source_bound.rs:646` 附近作为 native note。这不是物理 Oracle 装载。

返工：Loader 不得把调用方 grade 与 `cross_table_consistent` 写成观测结果。grade 应来自获准 run 的字典或捕获审计。低于 T 的跨表记号按 AT-SRC-08 拒绝。Builder 的 `ready false` 与 Loader 的 trusted 行分开保留。

## F4 过期位点被写成 ok 并推进

`applied_pos_request` 在 `classify` 335 行解析为 1–38 位数字。`apply` 在 1489 行 `let _ = plan.applied_pos_request` 丢弃它。`audit_digest` 在 1490 行同样丢弃。

有父代且形状不是 snapshot 时，行位点取 `UPSERT_POS + index`（1328–1333）。`UPSERT_POS` 在 38 行，值为 `1_000_000`。批次水位取 `UPSERT_POS + upsert_rows - 1`（1380–1381）。

`0010_native_update_expand.sql:84` 允许 `position_state` 为 `ok`、`stale_risk`、`expired`。504 行插入字面量 `'ok'`。没有 `POSITION_EXPIRED` 分支。`sql_token`（1810–1829）没有过期 SQLSTATE。

探针 `expired-pos-child`：generation 2，`parent_source_generation` 1，`applied_pos "0"`，grade T。退出 0，`source_bound true`，`trusted true`。批次 `w77-child`，物理表 `pt_79fd8b92c8d1aec60909e36007630705b81ccb1958fbdcd0eb09d60a4d40`，payload digest `9e3fd1b2ea825688a4edf77be950e4f985227b717021b5e4c9cff622b5884e6f`。三行 `_oo_pos` 为 1000000、1000001、1000002，业务列仍是 101/10/a、102/20/b、103/30/c。`_oo.table_quality` 的 `position_state` 为 `ok`，`capture_method` 为 `c5_diff`，`delete_detection` 为 `reconcile`，`partial_txn_in_progress` 为 false。

旧代表 `pt_891c…` 仍有 3 行，`_oo_pos` 为 0、1、2。两代表并存。这只说明旧表没有被这条子代删掉，不说明删除收敛或发布换表完成。

返工：请求位点过期时在封存前拒绝，不写批次，不把 `position_state` 写成 `ok`，不把水位推进到 1000002。AT-SRC-08 要求进入 `POSITION_EXPIRED`，旧位点不推进。

复现：`raw/harness/probe.py` 的 `expired-pos-child`。修复后 `w77-child` 行数为 0，水位不得变成 1000002。

## F5 重放身份不含 truth

`payload_digest`（1880–1916）覆盖 source、schema、table、generation、parent、batch、Arrow sha、tombstones、列名、config、schema digest、type digest、input digest。不含 truth、`consistency_grade`、`audit_digest`、`applied_pos`。

`execute`（171–181）在同一 source、schema、table、batch 上比较 digest。不同则返回 reason `OO010` 且不调用 `apply`。相同则 `replay_result`，不再比较 truth。

`replay-same` 把第三行 note 从 `a` 改成 `CHANGED`，其余字段相同。退出 0，`load_state replay`，digest 仍是 `5869b2bf…`。库内 note 仍是 a、b、c。没有半成品覆盖，也没有拒绝。

`replay-oo010` 的 tombstones 为 `["999"]`。该键不在 Arrow 行里，digest 变化。退出 1，reason `OO010`，stderr 空。批次仍是两条，`_oo.update_tombstone` 行数 0。

`replay-payload-tombstone` 的 tombstones 为 `["101"]`。该键与 Arrow 行重叠，`classify` 在 OO010 之前返回 `conflict`。退出 1，无新批次。

返工：digest 已覆盖的载荷变化继续以 `OO010` 拒绝且无批次。truth、grade、位点、audit 的变化也必须拒绝，不能返回 `replay`。重叠墓碑的 `conflict` 无批次可以保留，并在原因上与 `OO010` 区分开。

## F6 租约骨架不持有 fence

`jobs_lease_interface`（1720–1731）返回静态 JSON：`rust_claim_api` 为 `absent`，`target_fence_certified` 为 false，`call` 为 `not_invoked`，`jobs_lease_receipt` 为 null，`import_command` 为 `oo-pipeline import-native-update`。函数不连接数据库，不写 `jobs.leases`。

`crates/oo-jobs/src/lib.rs` blob `c0741097b3808cef75b46e21669f97c93aad74a2`，170 字节，sha256 `7ae677aa73c07a1a0946fc905a2d82de3c52e45d77088fb2ae3955aec4a57d29`。全文说明 claim、lease、scheduling 未实现。

成功 JSON 写明 target fence 是 loader generation advisory lock，不是控制租约。目标库 `to_regclass('jobs.leases')` 为 null。控制库没有 `_oo` 与 `jobs` schema，`to_regclass('_oo.update_batch')` 为 null。

导入在租约为 null 时仍把两批写成 `awaiting_publish`。null 不是 certified fence。

返工：提供对固定 jobs SQL 的真实 claim，并用旧 owner fence 挡住后写者；或者在租约缺失时拒绝封存。不得在 `jobs_lease_receipt` 为 null 时把 `target_fence_certified` 改成 true。

## F7 类型检查仍在，Oracle 物理类型未完

`bad-type-utf8` 把第一列声明为 utf8，Arrow 仍是 int64。退出 1，reason `bad_type`，批次 0。去掉按 engine 或 weight 的硬拒之后，这次类型检查还在。

作者包保留：Oracle 封存列 int2/utf8 对 int64 fixture 的物理装载退出 1，reason `bad_type`，无 Oracle 批次。本复审不把这次 utf8 探针记成 Oracle 产品装载完成。宽 NUMBER、decimal256、uint64、date、字符串键、pg range、秒 9 精度的显式损失没有在本复审重跑，保持 pending。

## F8 权限没有因 EXECUTE 而泄漏表 SELECT

`0010_native_update_expand.sql:603–607` 从 PUBLIC 与 `oo_pipeline_runtime` 收回 `_oo` 业务表的全部权限。618–624 行把 `native_update_*` 的 EXECUTE 授给 `oo_pipeline_runtime`。

探针登录角色 `oo_dl01w1_77_runtime` 以 `INHERIT TRUE, SET FALSE` 成为 `oo_pipeline_runtime` 成员。导入以该角色成功。没有追加表 SELECT。

确认查询：runtime 与 stranger 对两张物理表和 `_oo.update_batch` 的 SELECT 均为 false。runtime 对 schema `_oo` 的 USAGE 为 true。USAGE 不授予表 SELECT。

`stage_select_granted` 为 null。`stage_probe`（1625–1631）在角色 `oo_stage_probe` 不存在时返回 `Ok(None)`。本库没有创建该角色。null 表示探针没测到 stage 角色，不表示 SELECT 已授予。

## 作者失败保留

以下保持作者原文中的失败，本复审不重跑、不改判通过：

- 无 `admission_path` 的 ClickHouse c4b、Doris b2、fixture gen1/gen2、mongo-canonical：`source_bound false`。它们是目标上的原生批次，不与本次 `oo_u06_bind` 的 trusted 行混成全单元格证明。
- 裸 double 拒绝 `jsonb_double_unencoded`。规范 `{"weight": {"$numberDouble": "41.25"}}` 落入 jsonb 且 `source_bound false`。
- 产品信封退出 1，code `oo.pipeline.jsonb-document-import/v1/cross_source`。catalog 表 `public.docs` 不是 dataset `oo_w141.shipments`。隐藏字段 code `oo.document.bson-jsonb/v1/field.scope`。16 行 `baseline.seal.json` 仍是源封存，物理 COPY 未做。`document-profile` 20 列且 `ready false`。
- StarRocks 缺窗口时 `upsert_window_unsupported`，无批次。
- `cell_mismatch`、作者侧 `OO010`、墓碑键冲突、旧 generation 拒绝、`fixture-mirror-missing` 的 `OO030` 均无批次行。
- prove194 的 Docker 退出 0 与 Python 退出 1 同时存在。follow 的 `summary.json` 用 `or True` 把产品信封标成 ok，stdout 是 `cross_source`。
- rebuild194 的 15 个测试早于生产者绑定测试。`/tmp/oo-u06-attempt2-198/version.txt` 是 1.98.1。ELF `e2e8c5ab…` 是 prove194 的 pipeline，不是本提交的 ELF。
- 模型目录空，S5 未调用。Bearer 目录未读，本复审不复制 secret。

## 已核对且不构成通过的构建事实

`oo-core` 两个 source-bound 测试与 `pg-builder` 五个 source_contract 测试在补齐 oracle live-chain catalog 后退出 0。`pg-builder` debug ELF 已生成。这些测试读的是封存字典夹具，不给 Loader 的 `trusted true` 背书。

Clippy `-D warnings` 对 `oo-pipeline` lib 与 bin 退出 0。`crates/oo-connectors/vendor/sqlx-postgres/src/message/flush.rs:11` 与 `message/mod.rs:76` 的 `#[expect(dead_code)]` 未满足，警告留在 `raw/phases/pipeline-clippy.log`。
