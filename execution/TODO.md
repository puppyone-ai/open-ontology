# 完整产品 TODO 与派发入口

更新：2026-10-16。[索引](INDEX.md) / [状态更新协议](UPDATE-PROTOCOL.md)。由 [task-ledger.json](task-ledger.json) 生成；修改台账后运行 `python3 execution/scripts/render_todo.py`，CI/协调核对使用 `--check`。

本清单勾选表示**整个父项正式退出**，不是开发组件是否存在。未勾项可能已交付多个组件；状态“待签收对账”不能解释为尚未开发或开发完成率为零。开发子项与证据见[验收计数](../specs/open-ontology-acceptance-counting.md)，历史交付见[总台账](../specs/open-ontology-development-ledger.md)。87个SP、14个DBX与8个DBP分别统计，不合并分母。

完整要求、输入/交付/退出条件从[主Plan](../specs/open-ontology-v0.1-plan.md)逐卡引入机器台账；实际派发前还须冻结owner、接口SHA、路径、测试命令、资源和完整能力卡。未填owner/handle表示未派发，不从OS进程数推断。

产品交付是Ontology生产/审核/发布系统、角色Pack和外接接口。内置Pi用于体验和内部验证；外接使用不依赖Pi会话，具体支持协议/客户端/版本以实际验收为准。

交付顺序见[PRODUCT-LOOPS](PRODUCT-LOOPS.md)：当前DL01，整版按序签收，开发按固定依赖/独占文件/资源余量并行；与SP共用实现和证据。五版结束必须同时完成80必交SP/14DBX/15阶段，SP15七项明确后续扩展单列。

## 当前已验收里程碑

- [x] PG-DRAFT-100K：十万行物流PG复制→真实Builder→草案Pack→Pi六问→Gateway→关闭；[固定复审](../execution/reviews/gpt-minimal-closure-fixed-review.md)。**未审核/未发布草案；不自动完成父项、S9/S10或支持矩阵准入**。

## 当前冻结批次

- [execution/batches/native-luna-full-source-20261015/mysql-mainline-batch-reader-dispatch.json](batches/native-luna-full-source-20261015/mysql-mainline-batch-reader-dispatch.json)；实际运行句柄见CONTROL/JSON，批次状态只按退出/证据更新。
- DL01-W1-01 / matrix：development；development_child_accepted；Plan DBX-W01；固定依赖 577160c22de6。
- DL01-W1-02 / admission：development；native_waiting_dependency；Plan DBX-W02；固定依赖 577160c22de6。
- DL01-W1-03 / capture：development；development_child_accepted；Plan DBX-W03；固定依赖 577160c22de6。
- DL01-W1-04 / driver：source_product_development；author_full_unpublished_preview；Plan DBX-W04；固定依赖 577160c22de6。
- DL01-W1-05 / semantics：development；historical_terminal_ownership_transferred；Plan DBX-W05；固定依赖 577160c22de6。
- DL01-W1-06 / mysql：development；completed_root_verified_owned_fixture_mainline_integration；Plan SP03-W02；固定依赖 0b27b409a6eb。
- DL01-W1-07 / suite：development；author_complete_deferred_connected_review；Plan DBX-W14；固定依赖 577160c22de6。
- DL01-W1-08 / mariadb：target_preparation；setup_dependency_review_passed；Plan DBX-W06；固定依赖 577160c22de6。
- DL01-W1-09 / mssql：source_product_development；held_for_shared_stability；Plan DBX-W08；固定依赖 577160c22de6。
- DL01-W1-10 / oracle：target_preparation；root_verified_complete_owned_fixture_reference；Plan DBX-W09；固定依赖 29e3afd88d7c。
- DL01-W1-11 / clickhouse：target_preparation；author_preparation_runtime_review_pending；Plan DBX-W12；固定依赖 577160c22de6。
- DL01-W1-12 / doris：target_preparation；setup_dependency_review_passed；Plan DBX-W12；固定依赖 577160c22de6。
- DL01-W1-13 / starrocks：target_preparation；setup_dependency_review_passed；Plan DBX-W12；固定依赖 577160c22de6。
- DL01-W1-14 / mongodb：target_preparation；setup_dependency_review_passed；Plan DBX-W13；固定依赖 577160c22de6。
- DL01-W1-15 / opensearch：target_preparation；setup_dependency_review_passed；Plan DBX-W13；固定依赖 577160c22de6。
- DL01-W1-16 / pg-fixed：reconciliation；reconciliation_reviewed；Plan SP03-W01；固定依赖 577160c22de6。
- DL01-W1-17 / sp02-fixed：reconciliation；reconciliation_reviewed；Plan SP02-W01；固定依赖 577160c22de6。
- DL01-W1-18 / selfbuild-fixed：critical_review；review_request_changes；Plan SP07-W01；固定依赖 13ad15b4bc66。
- DL01-W1-19 / external-fixed：reconciliation；reconciliation_reviewed；Plan SP09-W01；固定依赖 577160c22de6。
- DL01-W1-20 / operations-fixed：reconciliation；reconciliation_reviewed；Plan SP14-W01；固定依赖 577160c22de6。
- DL01-W1-21 / operator-preview：development；typed_consumer_jsonb_author_complete；Plan SP09-W02；固定依赖 577160c22de6。
- DL01-W1-22 / clickhouse-review：independent_review；setup_dependency_review_passed；Plan DBX-W12；固定依赖 daff73456797。
- DL01-W1-23 / starrocks-review：independent_review；setup_dependency_review_passed；Plan DBX-W12；固定依赖 970311b5872a。
- DL01-W1-24 / matrix-review：independent_review；fixed_review_passed；Plan DBX-W01；固定依赖 29597079f7fe。
- DL01-W1-25 / admission-review：independent_review；fixed_review_passed；Plan DBX-W02；固定依赖 fabc471609ee。
- DL01-W1-26 / capture-review：independent_review；fixed_review_passed；Plan DBX-W03；固定依赖 280bc35da56b。
- DL01-W1-27 / suite-review：independent_review；review_request_changes；Plan DBX-W14；固定依赖 76928401921e。
- DL01-W1-28 / oracle-review：independent_review；setup_dependency_review_passed；Plan DBX-W09；固定依赖 d92fb9ac5efa。
- DL01-W1-29 / mariadb-review：independent_review；setup_dependency_review_passed；Plan DBX-W06；固定依赖 ed892b5eba9a。
- DL01-W1-30 / doris-review：independent_review；setup_dependency_review_passed；Plan DBX-W12；固定依赖 d93d595fa417。
- DL01-W1-31 / mongodb-review：independent_review；setup_dependency_review_passed；Plan DBX-W13；固定依赖 b24610ca75a7。
- DL01-W1-32 / opensearch-review：independent_review；setup_dependency_review_passed；Plan DBX-W13；固定依赖 28fd98240d28。
- DL01-W1-33 / operator-preview-review：independent_review；fixed_review_passed；Plan SP09-W02；固定依赖 f2357a026033。
- DL01-W1-34 / oracle-driver-preflight：target_preparation_extension；preflight_review_passed；Plan DBX-W09；固定依赖 577160c22de6。
- DL01-W1-35 / driver-review：independent_review；fixed_review_passed；Plan DBX-W04；固定依赖 7de78381be5f。
- DL01-W1-36 / semantics-review：independent_review；fixed_review_passed；Plan DBX-W05；固定依赖 b1ffb079d7be。
- DL01-W1-37 / suite-repair-review：independent_review；fixed_review_passed；Plan DBX-W14；固定依赖 876b3b916d6f。
- DL01-W1-38 / budget-report：development_parent_extension；historical_terminal_ownership_transferred；Plan DBX-W02；固定依赖 577160c22de6。
- DL01-W1-39 / mariadb-driver-preflight：source_product_development；author_complete_deferred_connected_review；Plan DBX-W06；固定依赖 577160c22de6。
- DL01-W1-40 / clickhouse-driver-preflight：source_product_development；author_full_unpublished_preview；Plan DBX-W12；固定依赖 577160c22de6。
- DL01-W1-41 / mongodb-driver-preflight：source_product_development；author_full_unpublished_preview；Plan DBX-W13；固定依赖 577160c22de6。
- DL01-W1-42 / oracle-driver-review：independent_review；preflight_review_passed；Plan DBX-W09；固定依赖 645d37badf7a。
- DL01-W1-43 / doris-driver-preflight：source_product_development；paused_terminal_user_goal_pause；Plan DBX-W12；固定依赖 29e3afd88d7c。
- DL01-W1-44 / opensearch-driver-preflight：source_product_development；author_complete_deferred_connected_review；Plan DBX-W13；固定依赖 577160c22de6。
- DL01-W1-45 / starrocks-driver：source_product_development；completed_author_preview；Plan DBX-W12；固定依赖 577160c22de6。
- DL01-W1-46 / postgres-product：source_product_development；completed_author_preview；Plan SP03-W01；固定依赖 577160c22de6。
- DL01-W1-47 / mariadb-product：source_product_development；merged_into_source_owner；Plan DBX-W06；固定依赖 577160c22de6。
- DL01-W1-48 / clickhouse-product：source_product_development；merged_into_source_owner；Plan DBX-W12；固定依赖 577160c22de6。
- DL01-W1-49 / doris-product：source_product_development；merged_into_source_owner；Plan DBX-W12；固定依赖 577160c22de6。
- DL01-W1-50 / mongodb-product：source_product_development；merged_into_source_owner；Plan DBX-W11；固定依赖 577160c22de6。
- DL01-W1-51 / opensearch-product：source_product_development；merged_into_source_owner；Plan DBX-W13；固定依赖 577160c22de6。
- DL01-W1-52 / budget-connected-review：independent_review；component_review_approved_with_cancel_audit_gap；Plan DBX-W02；固定依赖 6590cff4acc1。
- DL01-W1-53 / oracle-whole-fixed-review：independent_review；review_return；Plan DBX-W04；固定依赖 7788327ef17a。
- DL01-W1-54 / mysql-whole-fixed-review：independent_review；development_child_review_approved；Plan SP03-W02；固定依赖 e4f9131c6c36。
- DL01-W1-55 / mariadb-whole-review：critical_review；review_approved_complete_experimental_source；Plan DBX-W06；固定依赖 b4448f7391aa。
- DL01-W1-56 / native-binding-review：critical_review；review_approved_boundary；Plan SP03-W02；固定依赖 e304a6adeedb。
- DL01-W1-57 / opensearch-full-source-review：critical_review；review_return；Plan DBX-W13；固定依赖 239d73985a0a。
- DL01-W1-58 / doris-full-source-review：critical_review；review_return；Plan DBX-W12；固定依赖 319ab9f72410。
- DL01-W1-59 / mongodb-full-source-review：critical_review；review_return；Plan DBX-W13；固定依赖 1761c6958ef8。
- DL01-W1-60 / clickhouse-full-source-review：critical_review；executor_quota_exhausted；Plan DBX-W12；固定依赖 f60a2f6db7a5。
- DL01-W1-61 / full-source-review-46：critical_review；review_return；Plan SP03-W01；固定依赖 fa69cd2cb7d8。
- DL01-W1-62 / full-source-review-04：critical_review；review_return；Plan DBX-W04；固定依赖 ce47918bb274。
- DL01-W1-63 / full-source-review-45：critical_review；executor_quota_exhausted；Plan DBX-W12；固定依赖 ec6c90c7be1b。
- DL01-W1-64 / full-source-review-43：critical_review；fixed_review_passed；Plan DBX-W12；固定依赖 66c7d5bd24bd。
- DL01-W1-65 / full-source-review-46：critical_review；review_accepted；Plan SP03-W01；固定依赖 4dceeb495。
- DL01-W1-66 / opensearch-full-source-review：critical_review；fixed_review_passed；Plan DBX-W13；固定依赖 e582db56ce2a。
- DL01-W1-67 / full-source-review-04：critical_review；review_request_changes；Plan DBX-W04；固定依赖 fe6aa3ba4。
- DL01-W1-68 / full-source-review-45：critical_review；fixed_review_passed；Plan DBX-W12；固定依赖 e8a3e4c6085f。
- DL01-W1-69 / clickhouse-full-source-review：critical_review；fixed_review_passed；Plan DBX-W12；固定依赖 9e95d6bdb4d0。
- DL01-W1-70 / mysql-full-source-review：critical_review；review_return；Plan SP03-W02；固定依赖 84e123025c52。
- DL01-W1-71 / full-selfbuild-yaml-maintenance：shared_complete_capability；native_running；Plan SP07-W01；固定依赖 13ad15b4bc66。
- DL01-W1-72 / public-gateway-sdk-cli-complete：shared_complete_capability；author_complete_deferred_connected_review；Plan SP09-W01；固定依赖 8405b4585。
- DL01-W1-73 / ontology-publish-maintenance：shared_complete_capability；author_shared_gateway_delivered；Plan SP08-W04；固定依赖 e2bd526e9617。
- DL01-W1-74 / capture-jobs-schedule-lease：shared_complete_capability；native_running；Plan SP04-W06；固定依赖 1c4797e2f846。
- DL01-W1-75 / publish-maintenance-fixed-critical-review：critical_review；critical_review_request_changes；Plan SP08-W04；固定依赖 699ca96df609。
- DL01-W1-76 / public-gateway-fixed-critical-review：critical_review；critical_review_request_changes；Plan SP09-W01；固定依赖 91031011238c。
- DL01-W1-77 / native-loader-fixed-critical-review：critical_review；critical_review_request_changes；Plan DBX-W05；固定依赖 f0060dfbc217。
- DL01-W1-78 / customer-mysql-readonly-validation：real_customer_readonly_validation；author_readonly_probe_terminal；Plan DBX-W06；固定依赖 ebae1fa23。
- DL01-W1-79 / shared-native-source-product-loader：continued_shared_product_capability；native_running；Plan SP03-W02；固定依赖 56bcab6b8d85。

## 最近下一步与当前派发

- [ ] 对账SP既有固定证据：保留已验组件，标出各父项的剩余退出条件；不得将本次清单重置为“全未实现”。
- [ ] DBX-W01/W02/W03/W04/W05/W14：先冻结共享schema/ABI/适用目标ADR及开发子验收分母，再释放消费者。
- [ ] 优先真实MySQL/PG目标，具备依赖后并行SQL Server/Oracle批量→本地PG→实际Builder/Pack/Pi问答；协议兼容目标各自证明。
- [ ] 正式S9审核/S10分类与YAML→overlay→Git版本→发布→Pack贯通；草案工具和六问不能替代。
- [ ] SP09/SP13外接验收：非内置Pi的真实调用方取得同一发布Knowledge/Pack，经声明支持的CLI/HTTP/SDK/MCP核对数据、权限和审计；不要求内置Pi会话、不冒称所有harness已验证。
- [ ] 当前DL01跨SP领取完整十源预览子能力；后续独立卡按固定依赖、独占文件及资源余量并行，不挤占当前关键路径；各版按序签收，既有模块先对账；前端仍仅master-task三个固定会话。
- [ ] 每次转态同步CONTROL/运行快照、任务台账、开发计数、主Plan和CHANGELOG；Grok已获准新派或续跑，GPT例外范围及资源限制按最新用户指令执行。

## P0 数据库扩展父项（14项）

DBX状态仅表示计划/正式签收；支持矩阵按目标×版本×变体×驱动×能力独立定级，当前没有verified目标。

- [ ] **DBX-W01 — 支持矩阵与准入框架**：dispatched；owner：Codex / DL01-W1-01；完整签收：DL05。
- [ ] **DBX-W02 — 能力模型与源端写入分类**：dispatched；owner：Codex / DL01-W1-02；完整签收：DL05。
- [ ] **DBX-W03 — 捕获目录、一致性与保留窗口**：repair_running；owner：Codex / DL01-W1-03；完整签收：DL05。
- [ ] **DBX-W04 — 外部驱动worker与桥**：dispatched；owner：Codex / DL01-W1-04；完整签收：DL05。
- [ ] **DBX-W05 — SourceCatalog、源语义与主链接线**：dispatched；owner：Codex / DL01-W1-05；完整签收：DL05。
- [ ] **DBX-W06 — MySQL家族扩展**：running_preparation；owner：Codex / DL01-W1-08；完整签收：DL05。
- [ ] **DBX-W07 — PostgreSQL家族扩展**：计划，待冻结/派发；owner：待派；完整签收：DL05。
- [ ] **DBX-W08 — SQL Server（含Azure SQL、RDS）**：pending_real_target；owner：Codex / DL01-W1-09；完整签收：DL05。
- [ ] **DBX-W09 — Oracle**：running_preparation；owner：Codex / DL01-W1-10；完整签收：DL05。
- [ ] **DBX-W10 — Db2、SAP HANA、达梦、神通**：计划，待冻结/派发；owner：待派；完整签收：DL05。
- [ ] **DBX-W11 — 云数仓与预算**：计划，待冻结/派发；owner：待派；完整签收：DL05。
- [ ] **DBX-W12 — OLAP、湖表与Trino**：running_preparation；owner：Codex / DL01-W1-11, DL01-W1-12, DL01-W1-13；完整签收：DL05。
- [ ] **DBX-W13 — 文档与键值源**：running_preparation；owner：Codex / DL01-W1-14, DL01-W1-15；完整签收：DL05。
- [ ] **DBX-W14 — 套件、版本矩阵与自动降级**：dispatched；owner：Codex / DL01-W1-07；完整签收：DL05。

## 完整SP父项（87项，16个Phase）

### SP00 — 工程基础与共享契约

- [ ] **SP00-W01 — workspace、工具链与依赖方向**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP00-W02 — 类型与版本化契约**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP00-W03 — 规范值与脏值样本**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP00-W04 — 合成数据与攻击基础**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP00-W05 — xtask、测试环境与Evidence**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP00-W06 — 开源规范与供应链台账**：待固定签收对账；owner：待派；完整签收：DL05。

### SP01 — P1–P16 风险原型与决策冻结

风险原型Phase：按主Plan的P1–P16逐原型证据和Phase退出条件对账，无额外SP父项；不新增分母。

### SP02 — 控制面、任务与凭证

- [ ] **SP02-W01 — 控制库与jobs**：reconciled_pending；owner：Codex / DL01-W1-17；完整签收：DL05。
- [ ] **SP02-W02 — config与Git**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP02-W03 — validate/plan/apply**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP02-W04 — Secret与mTLS**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP02-W05 — OIDC与管理RBAC**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP02-W06 — Collector与反馈receiver**：待固定签收对账；owner：待派；完整签收：DL05。

### SP03 — 只读连接器与类型

- [ ] **SP03-W01 — PostgreSQL接入**：reconciled_pending；owner：Codex / DL01-W1-16；完整签收：DL04。
- [ ] **SP03-W02 — MySQL接入**：dispatched；owner：Codex / DL01-W1-06；完整签收：DL04。
- [ ] **SP03-W03 — 块规划与有界读取**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP03-W04 — 类型与PositionCodec**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP03-W05 — catalog与注册控制API**：待固定签收对账；owner：待派；完整签收：DL04。

### SP04 — CDC、快照与可恢复Loader

- [ ] **SP04-W01 — Debezium与事务解码**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP04-W02 — COPY、merge与sidecar**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP04-W03 — CDC-first快照与shadow**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP04-W04 — 大事务、暂停与隔离**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP04-W05 — 跨库恢复、租约与ACK/TRIM**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP04-W06 — full/incremental与删除收敛**：待固定签收对账；owner：待派；完整签收：DL04。

### SP05 — 文件、dump与对象生命周期

- [ ] **SP05-W01 — 上传与对象ACL**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP05-W02 — CSV/TSV/JSONL/Parquet**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP05-W03 — Excel**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP05-W04 — SQLite与SQL dump**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP05-W05 — 文件导入与数据策略**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP05-W06 — 下载、删除与保留**：待固定签收对账；owner：待派；完整签收：DL04。

### SP06 — PostgreSQL授权与Gateway

- [ ] **SP06-W01 — 数据库权限编译**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP06-W02 — SQL安全静态规则**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP06-W03 — token与准入限流**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP06-W04 — 查询执行与可证取消**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP06-W05 — 精确结果、质量与freshness**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP06-W06 — 准入审计与completion**：待固定签收对账；owner：待派；完整签收：DL04。

### SP07 — Self-build、审核与角色Pack

- [ ] **SP07-W01 — OOS与overlay**：reconciled_pending；owner：Codex / DL01-W1-18；完整签收：DL02。
- [ ] **SP07-W02 — S0/S1结构与画像**：待固定签收对账；owner：待派；完整签收：DL02。
- [ ] **SP07-W03 — S2–S4规则与专家卡**：待固定签收对账；owner：待派；完整签收：DL02。
- [ ] **SP07-W04 — S5结构化LLM**：待固定签收对账；owner：待派；完整签收：DL02。
- [ ] **SP07-W05 — 分类审核与owner证据**：待固定签收对账；owner：待派；完整签收：DL02。
- [ ] **SP07-W06 — 角色Knowledge Pack**：待固定签收对账；owner：待派；完整签收：DL02。

### SP08 — 业务语义、质量修复与发布

- [ ] **SP08-W01 — enums与业务日历**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP08-W02 — metric与语义生命周期**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP08-W03 — 实体与否定决定**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP08-W04 — G1–G13与原子可见发布**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP08-W05 — 对账与受控修复**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP08-W06 — 审核发布控制台**：待固定签收对账；owner：待派；完整签收：DL05。

### SP09 — SDK、CLI、导出与本地分析

- [ ] **SP09-W01 — Rust SDK**：dispatched；owner：Codex / DL01-W1-19；完整签收：DL03。
- [ ] **SP09-W02 — CLI**：dispatched；owner：Codex / DL01-W1-21；完整签收：DL03。
- [ ] **SP09-W03 — export完成协议**：待固定签收对账；owner：待派；完整签收：DL03。
- [ ] **SP09-W04 — TypeScript SDK**：待固定签收对账；owner：待派；完整签收：DL03。
- [ ] **SP09-W05 — oo-analysis与文件产出**：待固定签收对账；owner：待派；完整签收：DL03。

### SP10 — gVisor、出口代理与Pi

- [ ] **SP10-W01 — SandboxDriver与编排边界**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP10-W02 — 每会话网络与身份映射**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP10-W03 — 固定出口proxy**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP10-W04 — Pi真实适配**：待固定签收对账；owner：待派；完整签收：DL04。
- [ ] **SP10-W05 — 工具镜像、Pack与预热**：待固定签收对账；owner：待派；完整签收：DL04。

### SP11 — 会话释放、网页与飞书产品入口

- [ ] **SP11-W01 — session/turn持久状态**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP11-W02 — S1–S11释放与reaper**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP11-W03 — 记录、outputs与当前权限恢复**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP11-W04 — 网页对话**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP11-W05 — 飞书渠道**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP11-W06 — 管理运维页与M1安全启动**：待固定签收对账；owner：待派；完整签收：DL05。

### SP12 — 公开Bench与客户MVP

- [ ] **SP12-W01 — 数据、300题与gold**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP12-W02 — 冻结副本与独立会话运行**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP12-W03 — 评分、统计与报告**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP12-W04 — 客户接入与安全部署**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP12-W05 — dev、holdout与live闭环**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP12-W06 — MVP全旅程与签收**：待固定签收对账；owner：待派；完整签收：DL05。

### SP13 — MCP、外部harness与持续建议

- [ ] **SP13-W01 — MCP transport与包装**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP13-W02 — OAuth与token exchange**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP13-W03 — 外部token与角色能力**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP13-W04 — S6–S8增量建议与反馈**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP13-W05 — 可选harness适配**：待固定签收对账；owner：待派；完整签收：DL05。

### SP14 — 性能、恢复、运维与正式发行

- [ ] **SP14-W01 — 完整F与配额标定**：reconciled_pending；owner：Codex / DL01-W1-20；完整签收：DL05。
- [ ] **SP14-W02 — 完整Compose与health**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP14-W03 — 离线与供应链制品**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP14-W04 — 全状态备份恢复**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP14-W05 — 升级、回滚与应急**：待固定签收对账；owner：待派；完整签收：DL05。
- [ ] **SP14-W06 — 观测、渗透与release**：待固定签收对账；owner：待派；完整签收：DL05。

### SP15 — 可选扩展与下一阶段设计

- [ ] **SP15-W01 — Kubernetes与其他沙盒驱动**：待固定签收对账；owner：待派；完整签收：SP15后续扩展。
- [ ] **SP15-W02 — 新数据库连接器**：待固定签收对账；owner：待派；完整签收：SP15后续扩展。
- [ ] **SP15-W03 — Kafka buffer替代**：待固定签收对账；owner：待派；完整签收：SP15后续扩展。
- [ ] **SP15-W04 — 指标编译与外部语义格式**：待固定签收对账；owner：待派；完整签收：SP15后续扩展。
- [ ] **SP15-W05 — 列式加速与存储策略**：待固定签收对账；owner：待派；完整签收：SP15后续扩展。
- [ ] **SP15-W06 — 可复现构建**：待固定签收对账；owner：待派；完整签收：SP15后续扩展。
- [ ] **SP15-W07 — M4独立设计**：待固定签收对账；owner：待派；完整签收：SP15后续扩展。

## DBP风险原型（8项，单列）

- [ ] **DBP1**：Oracle LogMiner辅助表写入面能否收敛为预创建的单一表；OpenLogReplicator是否可只读部署；Oracle驱动路线（ODPI-C/官方纯Rust）可用性；既有结果待固定证据对账，不计开发父项。
- [ ] **DBP2**：SQL Server CDC的事务边界、LSN到事务的重建、副本上读取变更表的回放证明；既有结果待固定证据对账，不计开发父项。
- [ ] **DBP3**：Aurora/RDS/Cloud SQL故障转移后的位点与槽保留、读副本回放屏障（DEC-06）、开启binlog的开销；既有结果待固定证据对账，不计开发父项。
- [ ] **DBP4**：Snowflake/BigQuery变更读取窗口、dry-run估算精度、初始快照耗时与窗口的关系、成本；既有结果待固定证据对账，不计开发父项。
- [ ] **DBP5**：ODBC/JDBC桥的流式、取消传播、背压与内存（Db2、HANA、达梦）；既有结果待固定证据对账，不计开发父项。
- [ ] **DBP6**：MongoDB change stream事务与resume语义、稳定字段阈值标定、`doc`列保真；既有结果待固定证据对账，不计开发父项。
- [ ] **DBP7**：TiDB/OceanBase/GaussDB等兼容层的认证、位点与历史读行为；既有结果待固定证据对账，不计开发父项。
- [ ] **DBP8**：各引擎字符串/二进制键的二进制排序分块与索引可用性；既有结果待固定证据对账，不计开发父项。

## 外部门槛（不进入开发分母）

- [ ] **X01**：真实负责人/CODEOWNERS/安全联系；待外部核验。仅阻塞适用验收，不阻塞其他独立开发。
- [ ] **X02**：客户授权/分类/驻留/私有holdout及企业连接配置；待外部核验。仅阻塞适用验收，不阻塞其他独立开发。
- [ ] **X03**：受保护CI/签名晋级/环境权限；待外部核验。仅阻塞适用验收，不阻塞其他独立开发。
- [ ] **X04**：真实runsc部署证明；待外部核验。仅阻塞适用验收，不阻塞其他独立开发。
- [ ] **X05**：真实客户UAT和外部客户端；待外部核验。仅阻塞适用验收，不阻塞其他独立开发。

## 更新与签收规则

任务台账是当前派发/父项签收结构化入口；开发历史仍以原总台账、冻结计数及固定报告为准。转态填写真实owner/session/handle、资源和路径；作者交付、独立review、集成、发行分别记录，父项全部退出后才能status=accepted并勾选。本生成器拒绝无固定源码/证据/独立复审的accepted项，但不能替代人工审阅全部退出条件。

MySQL mainline module/feature and 5.7+8.4 updated fixture chains root verified; writer released. Static baseline freshness remains limited. Other sources and broad Goal paused; formal0/10.
