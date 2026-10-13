# DL01-W1-77 固定复审

判定：`REQUEST_CHANGES`。

复审对象是 `f0060dfbc217ec3cf1376dc7e873878acee88081` 的原生 Loader 与 source-bound 语义。该提交的祖先包含 `2e0bd098363ace48bda9b9127f5fabe6d3a1647d`、`8397fd5b984df364cc3638ffa1e69ea06e125d74`、`983577c9b89453b08ab4e1a02cab88df78c3e005`。`merge-base --is-ancestor` 三者均为 0。源码与依赖来自该提交的 `git archive`。工作区 HEAD `4fc6b91c801748acb6045d95ab0b135c2319f47d` 没有参与编译。

本复审不批准全源验收。GOAL 里的完整签收 0/10 与受控预览 6/10 保持原计数。真实模型 S5、Gateway 对话、production、published 角色、客户夜跑仍 pending。一份返工交回原 06 工程。不新增功能分母，不改被审代码、台账、root Cargo。

作者证据根 `evidence/b5ac4bc8-1872-4fe4-a56a-04a70f7103a2/DL01/native-physical-import/source-bound-loader` 的 `SHA256SUMS` 有 207 条。对 `f0060df` 的 git blob 重算 sha256，207 条全中，0 条不符。清单在 `source-inventory.txt`。

## 范围

独立固定复审。作者完成、集成长、发布就绪是三层不同的门。这里只判定固定提交上的原生装载与 source-bound 绑定。

编译与探针在 Linux 容器内完成。镜像 `rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`，`rustc 1.94.0 (4a4ef493e 2026-03-02)`，目标 `aarch64-unknown-linux-gnu`。Postgres `postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`，版本 17.10。两库例外已记录：目标 `oo_dl01w1_77`，控制 `oo_dl01w1_77_control`。源侧只用作者已提交的 `gen1.arrow`，sha256 `ea5c4ded359e549417da7af4e6ec651fb58109c3bfb857b5367c3b10bf5218a7`。没有写外来源，没有重启或暂停外来容器。

独立 debug ELF 与作者包内声称的 ELF 字节不同。作者 ELF 不在 git 中，本复审没有重算作者二进制。记录的是本次 archive 构建的 ELF，见 `binary-hashes.txt`。

## 决定性探针

安装 `oo-pipeline install-native-update` 退出 0。结果 digest `74a2db86560e6354142a447d886d22fb7bc19b6b53cf027439028b5aadd1d617`，等于 `0010_native_update_expand.sql` 的 sha256。

| 探针 | 退出 | JSON reason | 批次 |
|---|---|---|---|
| forged-file-grade-t | 0 | 空 | 写入 `w77-forged`，`source_bound true`，`trusted true` |
| expect-identity-ignored | 0 | 空 | 同一请求的 replay，不是独立期望失败 |
| replay-same | 0 | 空 | `load_state replay`，单元格仍是 a/b/c |
| replay-payload-tombstone | 1 | `conflict` | 无新批次 |
| replay-oo010 | 1 | `OO010` | 无新批次 |
| cross-source | 1 | `cross_source` | 无批次 |
| self-equal-hash | 1 | `digest_mismatch` | 无批次 |
| expired-pos-child | 0 | 空 | 写入 `w77-child`，`applied_pos 1000002`，`position_state ok` |
| bad-type-utf8 | 1 | `bad_type` | 无批次 |

这些拒绝的 stderr 都是 0 字节。`OO010` 来自 `native_update.rs` 的 digest 比较，进程退出 1，没有打出 PostgreSQL SQLSTATE。`sql_token` 能把服务端 `OO010`、`OO013`、`OO011`、`OO017`、`OO024`、`OO030`、`OO031`、`42P01`、`23514`、`22023`、`22003` 收成 reason。本次探针没有走到这些服务端 SQLSTATE。

第一次权限查询用了未加 schema 的物理表名，psql 退出 3，错误文本是 `relation "pt_891c…" does not exist`。随后对 `_oo.pt_…` 的查询给出 SELECT false。

## 判定摘要

必须返工：

1. 调用方自写的 receipt 文件，只要与装载文档字段相等、且 `declared_receipt_sha256` 等于该文件自身 sha256，就被标成 `trusted true` 并提交 3 行。`admission_expect` 写着 doris / `oo_u06_fix` / generation 9，文档写着 clickhouse / `oo_u06_bind` / generation 1。catalog 哈希 `cd`×32 没有对应打开的 catalog 文件。
2. 调用方 `consistency_grade T` 与 `cross_table_consistent true` 被写入 `_oo.source_watermark`。没有字典。
3. 请求 `applied_pos "0"` 被丢弃。子代表格 `_oo_pos` 写成 1000000、1000001、1000002，水位 `applied_pos` 写成 1000002，`position_state` 写成 `ok`。
4. 只改 truth 的重放返回 `replay`。digest 不含 truth、grade、audit、applied_pos。
5. `jobs.leases` 不存在，`jobs_lease_receipt` 为 null，`target_fence_certified` 为 false，导入仍然封存。

保持的拒绝，不构成产品通过：`cross_source`、声明哈希等于 schema digest 时的 `digest_mismatch`、`bad_type`、墓碑键 `999` 的 `OO010`、与 Arrow 键重叠的墓碑 `conflict`。旧代表物理表与子代表物理表同时各留 3 行。控制库没有 `_oo` 与 `jobs`。运行角色与陌生角色对两张物理表和 `_oo.update_batch` 的 SELECT 为 false。

作者包里继续保留、本复审不改判为通过的失败：Oracle 物理 int2/utf8 的 `bad_type`、Mongo `public.docs` 对 dataset `oo_w141.shipments` 的 `cross_source`、16 行产品 COPY 未做、StarRocks `upsert_window_unsupported`、无 `admission_path` 的批次 `source_bound false`、pg-builder `ready false`、空模型目录与未调用的 S5、`follow` 摘要里 `or True` 把产品信封标成 ok。

## REQ 与 AT

状态词：`rework` 本复审已看到门未过；`probe_held` 本次探针符合该条拒绝或隔离，AT 仍未关闭；`author_retained` 作者失败保留，本次未重跑；`pending` 未做，不能签过。

| ID | 本复审判定 |
|---|---|
| REQ-SRC-01 Connector / 只读体检 | pending。本次没有对活源跑 trait 与 AccessReport。 |
| REQ-SRC-02 AccessReport | pending。 |
| REQ-SRC-03 CDC / incremental / full 选择 | pending。探针里的 `c5_diff` 是调用方闭合记号。 |
| REQ-SRC-04 类型保真 | `probe_held` 仅 utf8 声明对 int64 Arrow 的 `bad_type`。Decimal256、uint64、date、uuid、blob、秒 9 精度没有在本复审重跑。 |
| REQ-SRC-05 上传状态机 | pending。不在本次装载面。 |
| REQ-SRC-06 解析进程隔离 | pending。 |
| REQ-SRC-07 replace / append / upsert | `probe_held` 旧代表保留、冲突墓碑无批次。发布换表未做。 |
| REQ-SRC-08 支持矩阵 | pending。本复审不晋升任何等级。 |
| REQ-SRC-09 能力声明与写入分类 | pending。driver digest 使用调用方常量 `ab`×32，没有打开 driver 报告。归入信任返工。 |
| REQ-SRC-10 C1–C5 | `rework` 的一部分。捕获方法是调用方记号，随 grade T 一起落库。不是连续捕获证明。 |
| REQ-SRC-11 驱动五类 | pending。信任路径没有调用 `verify_inputs` 去核对 driver 字节。 |
| REQ-SRC-12 semantic caveats | `probe_held` 仅 pg-builder 与 oo-core 的字典夹具测试。`unconstrained_number` 仍是 native note。`ready` 与 `partial_ready` 为 false。可调用语义页 pending。 |
| REQ-SRC-13 计量与预算 | pending。 |
| REQ-SRC-14 BSON / 文档映射 | `author_retained`。产品信封 `cross_source` 正确，16 行物理 COPY 未做。本复审未重跑 BSON。 |
| REQ-SRC-15 黑盒套件 | pending。 |
| AT-SRC-01 只读与权限 | `probe_held` 运行角色与陌生角色 SELECT false，schema USAGE true。活源语句审计 pending。 |
| AT-SRC-02 预算与取消 | pending。 |
| AT-SRC-03 类型双路径 | pending。单次 `bad_type` 不关闭本 AT。 |
| AT-SRC-04 键与排序 | pending。 |
| AT-SRC-05 脏值与重放 | `rework`。truth 从 `a` 改成 `CHANGED` 仍返回 replay。 |
| AT-SRC-06 矩阵外拒绝 | pending。 |
| AT-SRC-07 源端零写审计 | pending。本次源是只读 Arrow 文件，没有外来源审计。 |
| AT-SRC-08 位点过期与 T 级 JOIN | `rework`。过期请求被写成 `ok` 与 grade T。 |
| AT-SRC-09 删除收敛 | `probe_held` 重叠墓碑 `conflict`、无墓碑行。真实源删除收敛 pending。作者 Doris 墓碑观察保留为作者记录。 |
| AT-SRC-10 worker 与 digest | `rework` 与信任门同一项。崩溃与网络矩阵 pending。 |
| AT-SRC-11 caveat 全链 | `probe_held` 夹具测试 5 项在补齐 catalog 后通过。角色页 `ready false`。全链 pending。 |
| AT-SRC-12 云预算 | pending。 |
| AT-SRC-13 文档映射 | `author_retained`。隐藏字段 `field.scope` 与 20 列未签发页保留为作者记录。 |
| AT-SRC-14 声明与用例 | pending。 |
| AT-PL-01 快照屏障 | pending。 |
| AT-PL-02 墓碑与旧事件 | `probe_held` 两条墓碑探针无半成品。事务顺序与 GC pending。 |
| AT-PL-03 位点与旧 run | `rework`。请求位点 0 推进到 1000002。 |
| AT-PL-04 大事务 | pending。 |
| AT-PL-05 租约与旧 owner fence | `rework`。null lease 不能当 certified fence，导入仍封存。 |
| AT-PL-06 对账 | pending。`reconcile_status ok` 来自封存函数写入的调用方 truth，不是独立对账。 |
| AT-PL-07 TOAST | pending。 |
| AT-PL-08 LOGGED shadow | `probe_held` 旧物理表仍在，子代表另表 3 行。`load_state` 为 `awaiting_publish`。换表与从库 ACL pending。 |
| AT-PL-09 schema 变化 | `probe_held` 仅 `bad_type`。加列、drop、TRUNCATE pending。 |

Plan 对照：SP03-W02 的类型与位点模型、SP04-W01 到 W06 的快照、墓碑、位点、租约、对账，DBX-W04 的驱动与 DBX-W05 的 SourceCatalog / caveat / Builder，都按上表保持未签。DBX-W05 是本任务父项，父项保持 open。

## 构建退出

| 阶段 | 退出 | 含义 |
|---|---|---|
| pipeline-build | 0 | `oo-pipeline` debug，1m 48s |
| pipeline-clippy | 0 | `-D warnings`。vendor `sqlx-postgres` 仍有两条未满足的 `#[expect(dead_code)]`，不使该检查失败 |
| pipeline-trust-test | 101 | 编译 `num-bigint` 时 E0463，找不到 `num_integer`。断言未跑 |
| pipeline-trust-retest | 101 | `target_installer.rs:1920` 的 `include_str!` 找不到当时归档里缺的 `contracts/pipeline/full-import-v1.json` |
| pipeline-trust-final | 0 | 跑了 0 个测试，54 个被滤掉。过滤串没有选中 `producer_receipt_binds_the_document_not_the_expect`。不能记成该单测通过 |
| core-source-bound-test | 101 | 两个测试在 `source_bound.rs:754` 因归档缺 `catalog.json` 失败 |
| core-source-bound-retest | 0 | 补上 `evidence/992aa5d3-…/live-chain` 后 2 passed |
| pg-builder-test | 101 | 3 passed，2 failed。失败点 `source_contract.rs:499`，同样是缺 catalog |
| pg-builder-retest | 0 | 5 passed |
| pg-builder-build | 101 | `fraction` 0.17.0 因 `num-bigint` rmeta 报 E0463 / E0432 / E0599 |
| pg-builder-rebuild | 0 | dev，19.55s |

产品行为的证据是实库探针，加上退出 0 的 core 与 pg-builder 重跑。第一次 101 是归档或增量编译问题，原始日志留在 `raw/phases/`。
