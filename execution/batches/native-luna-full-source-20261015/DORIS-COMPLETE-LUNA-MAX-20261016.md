# Doris 完整产品闭环，MySQL 双版本参考签收后接续

当前唯一工程师 gpt-6-luna/max；长跑完成全部退出才交付，不子派、无组件审阅。Ponytail full：最小完整 Rust 修改，复用固定产品能力，卡住先 AnySearch 官方依据，保留失败，不盲试或重复 S5/模型。root 专有台账/Plan/矩阵/registry/远端。本卡覆盖旧 Sol 卡的模型与排期。

先读 execution/INDEX.md、GOAL.md、PRODUCT-LOOPS.md、CONTROL.md、TODO.md、UPDATE-PROTOCOL.md、.codex/skills/codex-grok/SKILL.md、Ponytail skill，再读原 docs/03-sources-and-ingestion.md、database-support/03-change-capture-and-consistency.md、04-pipeline.md、06-ontology-layer.md、17-rust-implementation.md、Doris 相关 Spec/Plan 原 REQ/AT/DBX/SP 退出；原完整卡 DORIS-C4-C5-COMPLETE-SOL-20261016.md 的方法/资源/安全/全部六项退出仍适用。本卡、当前原始文件优先于旧卡运行状态；无前端。

## 固定输入与可复用底座

root 已核 MySQL5.7+8.4完整稳定参考：execution/reviews/native-luna-full-source-20261015/mysql-family-complete-fixture-reference-intake.json。真实 C1、S0-S10/维护Pack/fixture发布/RO副本/Gateway CLI/随机2×2已证，4676文件SHA、49pins和实际新API ELF均核。只读使用该固定参考与私有包内源码/ELF/正常配方；不要修改运行的 MySQL 容器、配置、数据、令牌或任何旧证据。

原 Doris owner 已 completed，五个未提交未测试 WIP SHA root 再核一致，交接 /tmp/oo-doris-connection-whole/sol-stop-20261016/HANDOFF.json。原始失败/备份不得丢失，WIP不能直接冒完成。原源7表 source19/target31、C4分区及C5表、旧正确S5/对话路径均在交接。先实际核现况；旧0039未安装、无新worker。旧人工baseline未确认可自备独立目标，用正式 native-serve 从generation0建立，禁止手改旧epoch/ACK/发布映射。严格身份匹配可复用正确S5；身份变了只能在全无模型预检完成后做一次必要真实S5，不能冒缓存。产品模型 Sol low 原URL/key；已知当前Sol429额度冷却，允许直接用已授权 grok-4.7/low，记录真实身份。canonical工具请求轮无schema/人工token上限，final保留tools/auto+strict schema，已真实预检通过；不要重复错误synthetic。

## 唯一写权与资源

本源 tools/doris-preview/、testing/dl01-driver-preflight/doris/reader/；共享精确文件 tools/release-coordinator/src/native_publication.rs、target.rs，crates/oo-pipeline/src/target_installer.rs、native_update.rs（仅必要installer接线），tools/job-worker/src/native_schedule.rs，crates/oo-pipeline/migrations/0039_native_per_table_publication.sql（仅未安装新迁移），必要同路径inline tests。所有已安装0001–0038等字节不可改。除此共享文件先给root精确因果/路径，禁止抢写；MySQL已固定服务并释放writer。不要带入其他源WIP或根Cargo.lock。必须固定源码archive/每ELF来源，再真实使用，不能只编组件。

仅自备本源具名FE、target/standby、identity/API/release/GW/private端口。旧18441身份有效性需现查，允许复用正常control-identity配方自备本源身份前置；不得改MySQL/Oracle身份/cookie/会话。缺环境自行在owned容器准备，不向用户索要不存在数据。秘密仅私有0600文件，客户库只读授权不得扩大。

## 完整退出与验收顺序

1. Linux Rust1.94最小相关tests与真实接口冒烟先过。按原docs真实 C4 分区刷新/C5全量差分，诚实S0/B/删除语义，修正未完per-table发布和quiet协议仅在确需时；保持C1/T与同relation角色保护。
2. 正常六字段默认入口，用户不给recipe/out；实际scheduler→Rust capture/Arrow→admission→Loader→CAS后ACK，真实自定义周期，两个独立I/U/D批含删除/分区替换，quiet不伪增代，崩溃恢复不丢/不重。FE→目标所有批准表/列全量typed oracle0差异（空表/decimal/date/string key/FULL/FINAL正确）。新基线与旧证据区分，不重放旧DML。
3. 真正S0-S10标准YAML/Ontology，当前语义与真实表类型/映射/方法/一致性/删除caveat准确，经官方maintain进入effective OOS和role Pack；不可仅sidecar。MySQL教训：从实际knowledge读产物核说明，不只看JSON字段存在。合法保留S5，不重复付费。
4. 正常fixture审核发布、角色权限/版本fence/CAS/真实物理只读副本replay barrier/持久审计；Gateway实际连接复制的数据，消费当前Pack。严禁手工published/ACK/篡改receipt。
5. 先无模型全bank CLI对独立源gold/current freshness、internal/unselected deny、实际admitted cancel/57014/rollback/后query；后源gold/Pack身份冻结的两组新随机、互不重复语义、各2轮真实Agent经OOCLI公共工具自主查询。不得注入SQL/答案/凭据，不重抽避错、不调弱validator。最终facts逐项sourcegold+实际CLI cell+原QID admit/complete WAL。只对严格同source/Pack/gold的已正确完整前缀合法resume，失败保留；不反复盲撞。
6. 默认whole_source全steps0，官方同输入重入/pins实读；精确注册命令交root操作，其他engine rows不变。交固定SHA/源码/ELF/依赖/原始exit/全部失败/SHAs/真实live或安全terminal资源/续签与截止/REPORT/FINDINGS/HANDOFF。未全出wholefalse，不能短交接。root做决定性核验；统一独立审阅仍按十源Goal时点，formal0/10/原Plan计数不自改。

源码提交使用fcntl /tmp/oo-dl01-wave1-git.lock + 私有index/精确owned提交，禁止add-A/reset/stash/push。固定等待600秒（clock.sleep单调用≤60秒分段），退出立即通知；当前唯一Doris完整工程师，无需占15个空转槽。安全终态交完整闭环或不可解外部阻断证据，不把局部成功报完成。

## 2026-10-16 实际水位权限缺口的最小范围增补

Doris 唯一 writer 增加 `crates/oo-pipeline/migrations/0040_native_source_progress_read.sql`：经官方 installer 追加 `oo_pipeline_runtime` 的 `SELECT(source,source_generation)` 列级读取，现有 target_installer/native_update 仅必要安装接线。不得改已安装0000–0039，不得授整表/其他列/写权限；实际 loader 验两列成功、其他列/写拒绝。C5 复用现有 target_database（缺省保留 control），C4 缺省目标仍拒绝，C1路由不变。原42501/42P01失败保留，完整六项退出及模型/计数不变；这是范围补充，尚非产品通过。

## 2026-10-16 overlay 分区验证写权交接

Oracle已固定0722c7b4f并明确释放overlay.rs，root核clean与SHA50149bf5一致。Doris新增唯一writer `tools/selfbuild-control-api/src/overlay.rs`，仅verify_doris_observation及必要tests：将fixture字面量p202604改为认证封存request/scope、load及observed分区向量的真实关联，不删除分区/ACK/身份校验，不改Oracle函数。固定0722源码+必要own delta，不采Oracle cli/store WIP；原全部出口保持。

## 2026-10-16 C4 quiet 两列最小只读权限

root逐列实查：0008原十列SELECT已生效，quiet新用current_signature/partition_scope缺权限，非prior多行。Doris唯一writer新增 `crates/oo-pipeline/migrations/0041_native_partition_observation_read.sql`，只GRANT这两列SELECT给oo_jobs_runtime，经既有installer追加安装，0008/0039/0040不改。key_texts等其他列/写仍拒绝；以实际loader和quiet原SQL测成功，保留全部失败，同pending合法恢复。native_schedule未变无需重编worker。本确定因果覆盖此前暂无0041判断，不扩表写/owner。


2026-10-16 追加Doris唯一共享窄域：product_flow.py/test_onboard.py仅Doris source semantics规范化/14+2证据staging及测试；pg-builder/source_contract.rs仅project_doris_capture_methods在真实S5无source_semantics时从已认证来源生成最小对象+测试，MySQL/其他函数不动。边界、失败、输入/接口/测试/固定制品/完整退出见 execution/reviews/native-luna-full-source-20261015/doris-maintenance-shared-scope.json。成功S5 b_bb4093b86850c9a35fbce465001b2f6d复用，零额外模型；原整源门槛不减。


2026-10-16 独占接收public_surface.rs/query_executor.rs，仅真实Doris C4/C5 per-table质量、新鲜度必要窄适配/tests，C1/Oracle原分支保持。先最小方案，固定Linux测试/commit/ELF后真验；NULL全局位点不得捏成0，未知时间不造值。原整源退出不减。 原子移交证据/边界：execution/reviews/native-luna-full-source-20261015/gateway-oracle-to-doris-transfer.json。
