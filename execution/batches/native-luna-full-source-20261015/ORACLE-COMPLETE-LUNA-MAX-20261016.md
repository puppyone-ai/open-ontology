# Oracle C5 完整闭环，MySQL 双版本前置已验后独立接续

唯一工程师 gpt-6-luna/max，长跑全部退出后才交付，不子派、不单派组件review。Ponytail full：最小完整Rust改动，复用已有产品/配方/正确S5；卡住先AnySearch官方资料，保留所有失败、禁止盲试或模型空转。root独占治理/Plan/计数/矩阵/registry/远端。本卡覆盖旧Sol/Grok模型及旧共享写域。

## 先读与固定输入

execution/INDEX.md、GOAL.md、PRODUCT-LOOPS.md、CONTROL.md、TODO.md、UPDATE-PROTOCOL.md、.codex/skills/codex-grok/SKILL.md、Ponytail技能；原 docs/03-sources-and-ingestion.md、database-support/03-change-capture-and-consistency.md、04-pipeline.md、06-ontology-layer.md、17-rust-implementation.md、Spec与主Plan相关原REQ/AT/DBX-W09/SP04/SP07/SP09/SP13退出。再读 ORACLE-C5-COMPLETE-SOL-20261016.md 的原全部出口，但旧共享文件授权已撤回，以本卡为准。不要把旧HANDOFF的行号/错误当当前阻断，先核现行固定代码与无模型证据。

MySQL5.7+8.4完整稳定固定参考已root核：execution/reviews/native-luna-full-source-20261015/mysql-family-complete-fixture-reference-intake.json，source29e3afd88/当前协调6a33ebae0。现有已核源码/实际ELF原始包在该intake，包含Linux1.94、真实C1全链、当前维护Pack/正式fixture发布/RO副本/GatewayCLI/随机Agent；只读复用，MySQL服务/数据/令牌/配置/身份不得修改。

Oracle前任completed并释放writer，commit827c6c7a63；完整交接 tools/oracle-preview/HANDOFF-SOL-20261016.md 与 /tmp/oo-oracle-c5-sol-20261016/HOLD-RECEIPT.json（SHA3cb0b54c3cbeb16d4ad9bed168e1e2e7e239a1ee2cd62f948104ac9f0c336738）。550项原证据/源码archive先前root全核；当前g10、pin24、source oracle_oneclick_orders、awaiting_publish，scheduler禁用/active_tasknull；真实C5/S0/per_table，LogMiner默认关闭。原g8/g9实际I/U/D和同task lease2故障恢复已证，g9仅20cells0、不是g10对账。先导出并独立全量对账已存在g10，不重放旧成功DML。旧正确S5 build b_739b3a14d1167f50b5f48ae22a8317e2仅严格合法身份匹配可复用，不重写原响应/receipt。旧g9 config/source语义维护曾被正确拒，所有错误保留；当前源码可已解决部分旧故障，先核再改。

## 精确独占范围

仅 tools/oracle-preview/、testing/dl01-driver-preflight/oracle/probe/、tools/selfbuild-control-api/src/overlay.rs（真实Oracle运行观测/语义分离、认证C5维护验证/投影及必要原位tests）。其他共享代码只读，确需增加exactfile先给root因果。Doris唯一owner独占 native_publication.rs、target.rs、target_installer.rs、native_update.rs、native_schedule.rs/新0039等；不得触碰或采它的五个未验证WIP。Oracle已验证现有generic single-run发布足够；继续消费固定CLI/ELF，不为它改Doris multi-run实现。

源码构建使用已核MySQL fixed archive或明确固定commit+仅自己的精确delta，不能从脏共享工作树整树构建。所有已安装迁移字节不变，不加无必要迁移。自有Oracle、target/standby、API/release/Gateway/private端口；可正常配方自备本源identity前置，不能改共享cookie/其他源数据库或服务。客户47.104.150.9/scm5保持零写且不再尝试用户名/密码；全部新I/U/D只用owned fixture分离DBA身份。Secrets私有0600、不进Git/argv/log。root管理正常registry安装，仅给root可复核命令/49式pins，不自行改其他engine rows。

## 完整一次出口

1. 先真实核g10/配置/源账号权限/金标/所有批准行列，source→target全量typed0。原config canonical顺序只能在完整JSON逻辑等价验证后保留；不改旧config/epoch/ACK/原receipt冒复用。
2. 实际必要共享缺口为overlay.rs的Oracle admission/evidence/projection与semantic_basis：SCN/position/executed/quality runtime/evidence变化不能假成业务语义漂移，但方法/grade/delete/config/schema/type/role/PII变化仍须拒绝。只用认证封存源证据推导，不能信任caller任意字串。复用现有Rust接口，Linux1.94最小正负tests后固定API archive/ELF，不能仅编译说完成。
3. 真正S0-S10标准YAML/Ontology/current role Pack；正确旧S5合法复用，数据更新经官方maintain无需新S5。实际读取当前Pack核真实Oracle精度/空串NULL/DATE映射、C5/S0/删除方法与表一致，不能只有sidecar。若正式契约证明旧S5不可复用，先所有无模型流程预检完成，再一次必要真实S5；禁止偷改hash/伪cache。
4. 正常fixture审核/发布/CAS/权限/版本fence/持久审计，真实physical RO standby与replay barrier，Gateway查询复制的真实数据并消费当前Pack。恢复实际自定义周期native-serve/scheduler；旧g8/g9 capture/fault证据合法保留，另做一组新唯一I/U/D证明新数据自动经过Loader→CAS后ACK→maintenance→publication→当前RO Gateway ready，quiet无假进度，独立新全量oracle0。保留真实长驻服务、固定源码/配置/argv/ELF/截止；不以仅enabledflag/定时读取/手工maintain证明自主全链。
5. 模型前先全bank无模型CLI/sourcegold/current freshness/实际Pack说明正确，拒internal/unselected，实际admitted cancel持久57014/rollback/后查询成功。后冻结独立sourcegold+当前Pack身份，两组新随机、不重复语义、各2轮真实Agent使用任意合法OOCLI公共工具；禁止注入gold/SQL/凭据、重抽避错或弱化validator。四最终facts逐一源gold+实际CLI cells+原QID admit/complete WAL核，原失败保留。
6. 正常六字段默认入口type/host/port/database/user/secret_ref，不让用户带recipe/out。全部journal步骤0、whole完整、正常同输入重入与实际pins/registry对应。固定完整源码/ELF/依赖/真实模型身份及usage/原始exit/全部失败/SHAs，更新本源REPORT/FINDINGS/HANDOFF和实际live/terminal资源/续签/截止。其他engine rows不变。完成才通知root；缺出口wholefalse，不签formal源/父项/整版。

## 成本与执行

产品仍原URL/key，Sol low已知429冷却，可直接使用人类授权grok-4.7/low；20:32UTC刚有真实strict facts/evidence+tools业务对话成功，不要反复synthetic验证。canonical工具轮无response_format/人工token cap，final为strict facts/evidence schema并保留tools auto；原schema/coverage/QID校验不改。合法完整前缀仅同source/Pack/gold/原WAL可resume，不反复盲试。

提交仅精确owned路径，fcntl /tmp/oo-dl01-wave1-git.lock+private index/--only，禁止add-A/reset/stash/push或修改他源stage。无固定CPU/jobs/编译槽限制，按真实资源headroom；当前Docker16CPU/94.1GiB、107容器实用21.3GiB/约1.6核，两完整源有独立资源。普通clock.sleep600秒（单调用≤60秒分段）、退出即报；不短交接、只在完整闭环或确实不可解外部阻断交原始证据。独立总复审按人类十源Goal时点，root决定性核验/计数/签收。
