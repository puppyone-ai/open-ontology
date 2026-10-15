# Oracle C5 完整产品参考长跑

唯一负责人：待派发 gpt-6.1-sol / medium；禁止子派与组件审阅。Ponytail最小Rust完整改动，先读原docs/Spec/Plan再实现。只在完整闭环或真实不可解外部阻断时交付，保留全部失败。

## 固定输入与依赖

MySQL8.4完整参考已由root核实，见 execution/reviews/native-luna-full-source-20261015/mysql8-complete-fixture-reference-intake.json。正常六字段真实完成6步骤、C1同步/恢复、S0–S10/维护、fixture发布、真实只读副本、Gateway/CLI/两组随机两轮Agent；不是生产认证或其他源验收。固定source6a2e99d1714c5853ffbe37f0ca48e9a0494f1206与Gateway84f86555c170f0b5ba692c944911319834c3581c、逐组件ELF/hash见原固定包。MySQL5.7负责人仍独占C1相关文件，继续使用上述已固定ELF。

先读 execution/INDEX.md、GOAL.md、PRODUCT-LOOPS.md、CONTROL.md、UPDATE-PROTOCOL.md，.codex/skills/codex-grok/SKILL.md、ponytail5.2 full、docs/03-sources-and-ingestion.md、docs/database-support/03-change-capture-and-consistency.md、docs/04-pipeline.md、docs/06-ontology-layer.md、docs/17-rust-implementation.md及Spec REQ-SRC-10/AT-SRC-08/11、主Plan相关SP04/SP07/SP09/SP13/DBX-W09原退出；再读原完整卡 execution/batches/grok-connection-full-20261014/oracle-native-normal-reuse-current-full-fresh-context.md（旧Grok执行器已被当前Sol覆盖）。按当前代码及原始证据核实，不依赖旧报告完成声明。

## 唯一修改范围

- tools/oracle-preview/ 与 testing/dl01-driver-preflight/oracle/probe/，已有外来WIP保留并接续。
- 已经MySQL负责人明确释放：tools/release-coordinator/src/native_publication.rs、tools/release-coordinator/src/target.rs、crates/oo-pipeline/src/target_installer.rs，以及必要新增的pipeline migration（只追加，所有已安装迁移原字节不得改）。只为当前Oracle真实消费所需最小修复；同范围必要测试放现有模块。其余共享文件只读，明确缺接口立即具名请求root，而非私自扩域或绕校验。
- 自有Oracle/target/standby/API/证据cache资源；不得重启/修改MySQL8.4或5.7当前服务、身份、数据、cookies。需要共用资源先与MySQL唯一负责人确认。
- root独占台账/Plan/计数/矩阵/registry全局pins/remote，工程师不push、不改全局控制。

## 已知真实共享缺口

native_publication.rs多run receipt当前硬编码mysql-gtid/transactional/T与mysql lineage；target.rs多run调用 _oo.publish_native_c1_collection(0036)。Oracle按原docs C3/C5，默认LogMiner关闭，真实per_table/S0或B不能伪造T。先核Oracle实需单run还是多run；尽量复用既有合法generic single-run path，确需多run才添加最小真实低一致性发布路径，不能放宽现有C1验证或改装旧0036。冻结独立源码/archive/Linux1.94 ELF，已有MySQL采用固定旧ELF不盲用并发树。

## 完整输入/接口/测试/退出

自备真实Oracle fixture及全部E2E前置条件，复用已有实例与原数据。客户47.104.150.9:1521/scm5保持零写，不再用户名或密码尝试。平台只读账号，所有新I/U/D只在自有fixture由分离fixture身份操作。六字段(type/host/port/database/user/secret_ref)经正式source-onboarding普通默认入口，一次真实链路，不能自带private recipe/out当用户输入。

1. 按原Oracle C3/C5方法，通过实际官方scheduler+worker自动运行两组新唯一I/U/D及可配置周期、quiet无假进度、真实中断/恢复；证明Loader与CAS之后ACK，不仅enabledflag/定时读取/人工fire。全量typed独立源-目标oracle所有批准列/行0差异，删行不复活；源读账号无写，原精度/空串NULL/DATE caveat保真。
2. 正常S0–S10、标准YAML/Ontology与真实表一致，合法复用原已正确S5禁止重付费；当前数据变化经真实官方maintain产current Ontology/role Pack，绑定真实C5/S0及current版本，LLM原身份/usage准确保留。确需新S5先真实工具+strictJSON代理最小预检，产品模型gpt-6.1-sol low；原URL/key不变，grok-4.7授权备选。不可复用失配输入或把旧receipt重写新身份。
3. 经真实审核/角色/已支持fixture正式发布通道，使复制的真实数据可见于Gateway；实际RO standby/replaybarrier/当前Pack/权限，所有客户/生产gate不得伪造。Gateway连接复制的真实数据库，Pack仅语义/授权不是代替数据。
4. 无模型实际CLI preflight当前结果正确、freshness truthful、拒绝internal/unselected；admittedcancel持久57014/rollback与后查询成功。然后两组新随机seed互不重复语义、每组两轮真实autonomous Agent使用OO CLI公共tools；问题不硬编码每次相同，冻结独立源gold且不把答案/SQL喂模型。最终结构facts独立与源gold+实际CLI结果及原QID admit/completion WAL核对，不能仅exit0或问题调弱避开失败。
5. 六字段默认journal全步骤exit0、whole证明完整及正常registry/hash对应实际调用；固定完整源码SHA/archive、逐运行ELF/依赖/fixture/model身份、原始exits/失败/usage、最终HANDOFF/REPORT/FINDINGS及资源留存回执。小问题延后，致命数据/权限/出处问题必须修。任何缺条件保持wholefalse，不冒充正式源通过。完成才通知root，不短交接。

提交只精确本卡owned paths，fcntl /tmp/oo-dl01-wave1-git.lock，保留foreign staged/working，禁止git add -A/reset/stash。卡住先AnySearch官方最佳实践，禁止重复盲试；API无效时原文回执+官方来源，不泄露secret。普通无新工作等待clock.sleep600秒（受单调用60秒上限分段），有退出即时通知root。不得为凑并行空转。
