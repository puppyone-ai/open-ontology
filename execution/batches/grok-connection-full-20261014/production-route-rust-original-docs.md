# 2026-10-14 最新直接用户：Rust生产路线与原逐源捕获方法

用户明确指出MySQL普通native路线C5偏离原C1 binlog CDC，要求作者重读原docs/Spec/Plan，业务核心尽量Rust，不偏产品方向。本卡是重申原退出，不新增/删门槛。最近快照/差分数据证据、Selfbuild/CLI/对话成功保留，只按实际方法标注；MySQL/MariaDB/PG的C5与Mongo standalone C5不能签收各自原C1目标。禁止sync:auto无声明静默降级、以重读快照/影子diff模拟日志事件，禁止把已见位点当已应用/ACK。

逐源方法如下（source route JSON同目录production-route-contract.json为机器入口）：
- mysql: C1 binlog CDC；GTID/source lineage, ROW/FULL, transaction boundaries, L0/Lend/applied/ACK。initial CDC-first snapshot or explicit batch capability only; C5 not C1 exit。必须读取 docs/database-support/04-database-profiles.md §2; docs/04-pipeline.md; SP03-W02/SP04-W01–03。
- mariadb: C1 MariaDB binlog；domain-server-sequence distinct codec/connector; transaction/applied/ACK。C5 is separate batch/reconcile proof, not original C1 completion。必须读取 profiles §2; DBX-W06; SP04。
- postgresql: C1 logical decoding；LSN, publication/slot DBA, transactional apply/applied/slot ACK。CDC-first snapshot initial only, preserve existing live slot。必须读取 profiles §3; SP04-W01–03。
- mongodb: C1 change stream on replica-set/sharded target；resume token opaque; clusterTime not conflated, txn proof before gradeT, POSITION_EXPIRED。standalone C5 is allowed standalone capability only; self-provision replica-set fixture for primary C1 route。必须读取 profiles §10; DBP6; DBX-W11。
- sqlserver: documented C1 native CDC or C2 Change Tracking；real LSN/CT version and retention/snapshot semantics, no fake T on CT。current real C2 allowed; C5 cannot replace declared CT/CDC。必须读取 profiles §4; DBX-W08/DBP2。
- oracle: verified batch C3/C5, S1 if same SCN proven；real SCN/snapshot guarantee only, C3 delete reconcile caveat。allowed batch; LogMiner C1 off without separate ADR/R1w/permissions。必须读取 profiles §5; DBX-W09/DBP1。
- clickhouse: applicable C4 parts/partition refresh or documented C3/C5；FINAL/delete async/partition epoch/CAS/ACK and actual grade。C5 allowed where applicable; no logCDC claim。必须读取 profiles §9.6; DBX-W10。
- doris: applicable C4 partition refresh or documented C3/C5；partition epoch/tablemodel; no external CDC for supported2.x/3.x。allowed method per table, not C1。必须读取 profiles §9.6; DBX-W10。
- starrocks: applicable C4 partition refresh or documented C3/C5；Primary/Unique/Duplicate semantics; real partition/deletion coverage。allowed per table, not C1。必须读取 profiles §9.6; DBX-W10。
- opensearch: C5 reconcile or C3 with explicit delete reconciliation；PIT/search_after, mapping schema, B actual consistency。C5 correct; no logCDC claim。必须读取 profiles §10; DBX-W11。

生产核心（连接器、日志解码/事务组装、位点/续读/去重、耐久spool、增量应用/恢复、调度、Builder/ontology编译、Gateway、OOCLI/MCP）按docs17依赖边界复用Rust实现/固定制品。Python/shell保留已有最小fixture/测试/编排胶水，不继续把产品算法加进脚本；不为语言偏好重写上游Debezium JVM/Pi Node或无关组件。snapshot导入、C1持续数据应用与schema/semantic维护分开；行DML只更新副本/动态freshness，不能每次重建ontology/调模型/手动刷新Pack才让agent恢复查询，schema/语义/授权变化才按原审核维护流程处理。

每源原工程师负责完整小闭环：前置与DBA只自有fixture（客户零写）→连接六字段普通Rust产品入口/现有Control接口→原捕获方法实际自动运行、insert/update/delete/事务/位点/断线恢复/过期拒绝→完整S0–S10和正确Yaml/保持curated→真实受权Pack/外部Gateway/OOCLI/实际正确多轮和QIDWAL/cancel。C1初始化要CDC-first L0/Lend/重放无缺口，不能从C5中途切换后凭flag补证；源账号实际零DML/DDL。周期是health/reconcile/batch的真实周期，C1常驻流不是定时全表扫描。C1/Pipeline接口确有缺失报exact Rust路径/签名/失败给sole共享writer，不越界另写共用文件。

独占/固定SHA/真实LinuxRust1.94/evidence/原失败/资源回执保持；原完整/正式0/10不改变，独立review仍统一全链后。所有作者在下一阶段先读本卡及原文相关章节，不能只读TODO摘要或历史报告。仍同UUID长跑，不新context/组件拆派，不在后台running时短完成通知。
