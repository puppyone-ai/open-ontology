# Doris真实完整C4/C5产品闭环（第三方法参考）

唯一gpt-6.1-sol/medium完整源工程师，不子派、不逐组件review。Ponytail full，最小Rust核心+既有薄fixture脚本；卡住先AnySearch官方资料，禁止盲试/重复正确S5/无用审查。完成下面全部退出才完整交付，不短交接、不因历史wholetrue签收。

## 先读与固定依赖

execution INDEX/GOAL/CONTROL/PRODUCT-LOOPS/UPDATE-PROTOCOL、.codex/skills/codex-grok/SKILL.md、原docs/03-sources-and-ingestion.md、database-support/03-change-capture-and-consistency.md、04-pipeline.md、06-ontology-layer.md、17-rust-implementation.md及Spec/Plan相关REQ-SRC-10/AT-SRC-08/11/DBX-W10/SP04/SP07/SP09/SP13原退出；再读 tools/doris-preview/HANDOFF.md，原完整卡 execution/batches/grok-connection-full-20261014/doris-current-native-default-whole-fresh-context.md（旧Grok规则被当前Sol覆盖）。

MySQL8.4完整fixed参考见 execution/reviews/native-luna-full-source-20261015/mysql8-complete-fixture-reference-intake.json：source6a2e99d17/GW84f86555c及逐组件ELF真实跑过，71hash0误，正常六字段6steps/wholetrue/真实C1自主恢复+S0–S10/维护Pack/fixture发布/ROstandby/GWCLI随机Agent。MySQL5.7仍同唯一工程师全链进行；Oracle另唯一工程师现有generic single-run C5，不再写下述共享文件。其他7源尚held，Doris为明确窄放行第三参考，不代表general source release。

## 独占边界

tools/doris-preview/、testing/dl01-driver-preflight/doris/reader/；继承WIP和所有原失败，不覆盖foreign文件。
MySQL与Oracle两负责人均明确无剩余修改/后台写、已释放：tools/release-coordinator/src/native_publication.rs、tools/release-coordinator/src/target.rs、crates/oo-pipeline/src/target_installer.rs、仅必要新增pipeline migration（所有已安装0001–0038等原字节不可改）。仅当前Doris真实消费确需才改，必要tests用原模块。其余共享pipeline/admission/jobs/Builder/API/verifier/source-onboarding为只读，需要exactfile先报root，无抢写。root独占治理/Plan/矩阵/全局registry/pins/远端；准备精确固定recipe和register_engine命令给root操作。
只用本源具名FE/target/API/GW资源；可自备必要独立readonlystandby/releaseDB/端口，不能改MySQL/Oracle数据/服务/身份/cookie。18441既有正常OIDC服务本轮已续期，private ~/.cache/oo-dl01w1-71/native-identity-private 五角色cookie可readonly引用，每次mutation取freshCSRF，不换共用文件。Secrets仅privatefile0600，不进argv/log/Git。

## 当前真实缺口（必须核原始文件，不以陈旧json推当前）

旧S5真实build b_1e205c67e1ad4861ca2b9438db5a0c44可在严格same identity下保留，真实7表/当前source19target31混合。part_day C4分区p202604，dup_events/uniq_acct/nz_obs C5，另3表未新观察不能伪T。当前whole.py/scheduled_producer.py有WIP，旧period0/1/2仅定时读取与手工Loader/ACK，根本未证明自主全链；source_semantics sidecar还需官方Ontology/Pack消费。原正确两组随机对话和S5留存，不为省事重跑或篡改。
共享真实潜在卡点：native_publication.rs多run硬编码mysql-gtid/transactional/T，target.rs0036同样仅C1。先核Doris单run是否合法覆盖完整7表；能用既有generic path就不加新代码。若确需multi-run/C4-C5混合，做最小真实per-table S0/B fenced发布，严保原C1 receipt验证/权限/CAS/barrier/audit，不能把非事务源改T、不能手SQL设published/ACK或删除受OO014保护映射。固定源码archive+Linux1.94 ELF后在本源真实消费，向root交可复用制品；MySQL/Oracle保留旧固定ELFs，不采并发树。

## 一次完整退出

1. 正常六字段(type/host/port/database/user/secret_ref)默认source-onboarding入口，无用户recipe/out，实际选择固定card，完整journal exit0。自备全部E2E前置，读账号零业务写，分离fixture身份新唯一I/U/D按需操作，不重放旧成功DML。
2. 官方native-serve/scheduler真实自定义周期→源Rust capture/typedArrow→admission→Loader→CAS后ACK；两组新唯一I/U/D包含物理删除/分区替换，quiet不伪增代/ACK，崩溃恢复不丢/重复。各表按原C4/C5如实披露，分区内原子替换与全量差分删除收敛。独立FE→目标全量typed oracle所有批准表/列（包括空表、decimal/date/stringkey）0差异，FULL/FINAL语义正确。
3. 完整S0–S10标准YAML/Ontology真实准确，原正确S5严格合法复用；每次当前源变动经官方维护使current source_semantics真正进入effectiveOOS/currentrolePack而非sidecar宣称，维护不用新S5。版本/角色/方法/一致性/删除 caveat真实绑定。
4. 正常审核与fixture发布流程，角色权限/版本fence/CAS/真实readonlystandby replaybarrier/持久审计，使Gateway连接复制的真实数据库，Pack用于语义授权。所有生产/客户cert gates保持真实未满足。
5. 先无模型CLI current数据/freshness准确及拒internal/unselected、真实admittedcancel durable57014/rollback/后query。再两组freshseed、互不重复语义、每组2轮真实autonomousAgent经OOCLI公共tools任意合法查询；模型前冻独立sourcegold，禁止喂gold/SQL，排除prior已问语义，不调弱题库避错。actual model gpt-6.1-sol low，原URL/key不变，grok-4.7授权fallback；记录真实response_model/usage。最终facts逐项gold+实际CLI结果+原QID admit/completionWAL核对。
6. default whole_source证明绑定本次真实调用/当前固定哈希，不能复制过去verdict冒新call；合法完整correctoutput复用要同source/Pack/gold/原WAL并披露。完整fixedSHA/sourcearchive/每ELF/deps/model身份、原始exits/失败/SHAs/资源回执及HANDOFF/REPORT/FINDINGS。一律wholefalse直到全出；formal仍0/10等待统一独立审阅，不计新开发分母。

源码提交精确owned paths+fcntl /tmp/oo-dl01-wave1-git.lock，禁止add-A/reset/stash/push。无广泛重构/额外依赖/组件审阅。普通观察clock.sleep600秒（单调用上限60秒分段），退出立即通知root。明确接卡后长跑到完整交付或真实不可解外部阻断；每条失败保留，不靠消耗模型反复验证已确定编译/权限问题。
