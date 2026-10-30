# ADR-029 Library：逻辑库、原子发布与不重导迁移

- 状态：已实现；匿名 PG17 上验证 36 表 / 3 来源、单 Pack 发布、adopt、重放、关闭后恢复、all-in-one 与真实物理从库。
- 前序：ADR-021（all-in-one）、ADR-023（先布局再复制）。

## Library 与布局

一个 Library 是逻辑数据库，shard sources 是传输细节；Agent 面向一个 Gateway、
一个角色和一个 Pack。一个分片也合法。`oo.library.manifest/v1` 包含完整 inventory、
sources、每分片 manifests、带 reason/upgrade_condition 的 exclusions、layout 和
scheduling；每个 inventory 身份只能归属一个来源或排除。

`python3 tools/source-onboarding/library.py plan/import/status/publish/adopt` 是产品入口。
初次 import 共用 source_layout/source_scheduler，不新增 Loader。默认每分片 16 表、
并发 2、错峰 10 秒；split 可选 prefix/domain/explicit/size；表 cap 1–4096、并发
1–32、错峰有限非负。size 按精确行数而非字节平衡。成本守卫可选，配置后缺失/
过期/超限暂停；重试固定 manifest digest，只执行失败或未完成表并累计既有成功。

## 发布协议与数据库事务

追加迁移 0061–0064，历史 0000–0060 不变。0061 引入库级发布/指针/成员；0062
补全关闭 fence 时的指针字段；0063 封闭输入；0064 兼容既有表 receipt 的已知可选
`rejects` / `position_lineage` 字段。每次安装校验精确前序 checksum，安装顺序追加
到 native_update；不重算历史 checksum。

发布入口 `_oo.publish_native_library(p_library jsonb,p_sha text,p_shards jsonb)`：

- 库 receipt 协议 `oo.pipeline.native-library-publication/v1`，封闭字段
  `library_id,manifest,manifest_sha256,expected_tables,shards,protocol`。
- manifest 为 `oo.library.membership/v1`：`library_id,sources,protocol`；sources 每项
  为 `source_id,schema,tables`。它是**发布成员清单**，不是带布局/排除的完整离线
  planning manifest；membership 的 canonical SHA 绑定实际参与发布的来源与表。
- 分片参数每项 `source_id,anchor_run,receipt_sha256,receipt`，内含原 per-table receipt。
  库 sha 是 `{library:p_library,shards:p_shards}` 的排序键、无空白 UTF-8 JSON SHA-256。
- 默认安装 ceiling：64 分片、4096 总表。管理员通过
  `_oo.configure_native_library_limits(max_shards,max_tables)` 显式配置正整数。
  单分片的原 per-source ≤32 限制不变，因此两个 ceiling 独立生效；例如超过
  2048 表还需提高分片 ceiling，不能仅提高总表数。
- 完整成员恰好一次；清单表集合精确匹配；同事务按 source_id 固定顺序锁全部
  source_watermark/table_freshness，逐分片调用既有 publish_native_per_table_collection。
  不复制其 generation、质量、ACK、覆盖校验。第 3 个分片失败，前两个也回滚。
- SECURITY DEFINER 固定 search_path；PUBLIC/runtime 无执行权限。库 receipt
  append-only；UPDATE/DELETE/TRUNCATE 守卫。相同当前 receipt 重放；被取代或
  当前 generation 不符的旧 receipt 拒绝，不能复活。

## Superseded、fence 与 ready

`native_library_publication` 保留不可变证据；`native_library_head` 为权威指针，状态
pending/ready/closed；`native_library_member` 限定来源只能属于一个库。成员集合
对已登记 library_id 固定，变更成员需要另行设计，不自动跨库搬迁。

adopt 允许 awaiting_publish 和已独立发布的混合来源；已有 per-source receipt 不
修改，通过 `native_library_source_status.status=superseded_by_library` 关联当前库
receipt。原 per-source SQL 保留；coordinator 拒绝对已 adopt 来源单独重新发布，
Gateway 对成员统一检查当前库指针和所有成员的当前 generation，而非信任旧源指针。

Coordinator 仍使用原 gate、candidate/version、Pack compiler、control CAS。
一个 candidate_target_binding 固定一个库 receipt；同目标事务发布所有分片并打开
所有目标 fence。旧分片角色先关闭，再启用整库角色；单来源旧路径不变。

目标提交不等于 ready：all-in-one 新只读会话检查可见向量、映射、每表 generation、
实际 SELECT；主从先等物理 replay，再在从库新只读会话检查，并等待 ready 指针
replay。任一步失败调用 native_library_fail_closed，一事务撤所有库表列权限、
关闭 fence、清 opened 指针并关闭库 head。不会撤回已经提交的不可变 receipt；
失败报告不宣称 ready。显式 `--recover` 只恢复同一 active candidate 的相同绑定，
重新全库关闭/授权/原子打开/新会话验证，不改数据或旧 receipt。

## 产品 adopt / publish / status

两命令共用 pipeline `native-library-prepare`：读取 Agent DB 当前表向量和既有
per-source receipt，不读源系统；在一个**最终回滚**的目标事务里调用真实发布
函数做预检，输出 native_mapping。然后调用现有 coordinator maintain-native-activate。
`--prepare-only` 不发布；`--recover` 调用全库维护恢复。需已审核并 staged 的单 Pack，
不能用本命令跳过 Self-build、审核或发布 gate。

`status --result` 是 runner 覆盖统计；`status --binding` 用真实数据库向量预检返回
覆盖数、generation_vector_verified、库 head 状态；二者不能混用。完整 planning
manifest 的排除条目计入 excluded，不计为 imported；仅传 membership 时统计范围
仅为成员表，不能声称它代表未提供的全库 inventory。

## 限制与对接

- 未连接客户系统；真实库 adopt 由协调者在私有 runbook 前置条件满足后执行。
- 本实现复用既有 test_scope 发布，不扩大为 production-ready 安全认证。
- 库 SQL ceiling 不解除 Builder/Pack 独立预算。coordinator visibility 默认 256，
  可显式 limits.max_datasets（正整数，最大 1048576）；当前基线 Builder 仍有
  64 dataset 限制，整库大 Pack 的支持由 BLD 合入后独立验收，LIB 不绕过 compiler。
- Gateway freshness 保持逐表 target_observations、多 source_ids，source_time=unmeasured，
  CROSS_TABLE_SKEW 保留；目标原子提交不是源端跨表快照。
- 失败后可能有已提交的 control candidate/数据 receipt，但全部库 fence/head 关闭；
  “回滚”仅指提交前 SQL 全有或全无，不能宣称跨 control/target 分布式事务回滚。

复现命令、通过数与基线：[LIB 验证记录](../database-support/library-lib-verification.md)。
