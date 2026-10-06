# 冻结验收计数与复用规范（OO-COUNT-001，2026-10-06）

本文件是 Plan 的执行附录。冻结当前优先纵向批次 M1-D 的24个开发验收单位；全项目87个父工作项和16个Phase仍全部保留。**17/24（70.83%）只表示此批次开发验收，不表示全项目开发完成率或可发行。** 后续数据库、文件、CDC等按单独冻结批次报告，不能纳入本批次分母后回填成功。

## 计数规则

开发单位完成要求：固定接口与范围、真实实现、Linux/MSRV局部测试与必要边界测试、固定提交独立批准、协调者核验原始退出及证据hash。已批准的有限接口范围按原review约束计数；发现强制返工时增加缺陷状态，不删除历史批准、不增加新功能单位；当前可用性另标受影响。修复、补报告、增加测试和重审均不增加分母或交付功能数。17个已验单位可包含同一功能的API与CLI，因接口/用户输出及验收不同；不能据此宣称16个独立产品组件。

新增/合并/删除单位必须先提交范围变更记录：旧分母、新分母、原因、对应Plan父ID、输入/输出、测试、旧证据迁移。禁止完成后再拆单位抬高完成率。历史35交付组件/41验收子项/10of34首次返工是历史吞吐计数，口径不同，不能用41除24。

全项目开发完成率须在其他批次接口分解冻结后汇总；目前显示“全量开发分母尚未冻结”，并展示M1-D和87父项逐项已有开发证据，不把87父项未正式退出显示为开发0%。父项正式签收仍遵守全部要求。

## M1-D：输入、输出和固定验收

每项测试默认复用所列独立报告中已批准的真实fixture/启动配方，新增验证针对实际接口与本次变化。后续实现卡必须再冻结具体文件与命令。

| ID | 开发单位 | 输入→输出 | 状态/独立证据 |
|---|---|---|---|
| M1-D01 | PG源连接与只读身份 | sourcefactory→readonly connection | 已验收，a164 |
| M1-D02 | PG full reader | selected catalog→bounded batches/RunComplete | 已验收，a149 |
| M1-D03 | 持久化物理映射 | logical identity→mapped lookup/replay | 已验收，a142 |
| M1-D04 | binary COPY编码 | typed batch→bounded exact COPY | 已验收，a148 |
| M1-D05 | 目标安装器 | empty target→version/readiness | 已验收，a152 |
| M1-D06 | 未发布全量导入 | source reader→awaiting_publish seal | 已验收，a159 |
| M1-D07 | import-pg命令 | trusted config→真实导入receipt | 已验收，a164 |
| M1-D08 | catalog-pg命令 | readonly source→captured catalog | 已验收，a166 |
| M1-D09 | catalog草案API | captured catalog→reviewable draft | 已验收，a157 |
| M1-D10 | 草案命令 | catalog artifact→sealed draft | 已验收，a160 |
| M1-D11 | 审核命令 | draft/curated decision→accepted/rejected artifact | 已验收，a163 |
| M1-D12 | 角色Pack编译器 | reviewed facts/visibility→bounded role pack | 已验收，a176 |
| M1-D13 | role-pack命令 | artifacts→role pack output | 已验收，a178 |
| M1-D14 | 列ACL安装 | mapping/role classification→column grants | 已验收，a173 |
| M1-D15 | SELECT AST准入 | trusted catalog/SQL→physical guarded query | 已验收，a179 |
| M1-D16 | live PolicyCatalog | actual mapping/ACL/OID→guard catalog | 已验收，a200 |
| M1-D17 | 实际PG查询CLI | trusted SQL→typed rows/audit/exit | 已验收，a208 |
| M1-D18 | 语义发布屏障 | reviewed version/DB seal→authorized published snapshot | 未验收，pending |
| M1-D19 | 自然语言CLI对话 | prompt/role pack→agent query turn/result | 未验收，pending |
| M1-D20 | 完整agent harness | session/pack/tools→actual agent RPC turn | 未验收，pending |
| M1-D21 | 公共身份准入与撤销 | token/mTLS→scoped admission/revoke | 未验收，pending |
| M1-D22 | 审计WAL与completion | admission/result→durable recoverable audit | 未验收，pending |
| M1-D23 | parser内核限制与监督 | untrusted request→restricted child/reap certificate | 未验收，pending |
| M1-D24 | session状态释放 | stop/restart/revoke→bounded resource release | 未验收，pending |

D15 parser内核隔离依赖D23；D16当前角色测试修复a206由a208验证；D17仍待复审；executor取消future缺陷a205由a210修复，不得用历史D15/D16批准宣称完整gateway可公共使用。D18–24只用合成数据可独立开发；真实身份/客户/受保护环境的外部核验列到对应发行门槛。

## 集成完成率（单独冻结4个节点）

| ID | 输入与完整输出 | 验收手段 | 状态 |
|---|---|---|---|
| M1-I01 | 实际源PG→完整导入/catalog→草案审核/Pack→列ACL/livecatalog→实际受控查询 | 同一真实数据链、独立值/身份/receipt对照、拒绝写与隐藏列、固定集成证据 | 未验收，a203进行中 |
| M1-I02 | I01实际产物→编译CLI单次/多轮SQL | 实际CLI消费同链产物，两角色结果/退出/audit，无人工替代中间产物 | 未验收 |
| M1-I03 | 已发布语义版本→自然语言CLI/真实harness/session释放 | 真实agent工具调用、权限拒绝、stop/restart/revoke资源闭环 | 未验收 |
| M1-I04 | 同一集成提交→Linux/MSRV双架构部署与回归 | pinned双架构完整相关矩阵/Compose启动/恢复，证据与源码一一对应 | 未验收 |

当前集成0/4；该0只表示四条完整链尚未验收，不否定17项开发交付。部分运行记录可标“已运行、待修复/待审”，不能冒充链完成。

## 发行准备度（冻结6个内部包，外部条件另列）

R01制品/双架构/SBOM与完整依赖政策，R02升级/迁移/回滚/备份恢复，R03安全/身份/审计/隔离完整矩阵，R04公开benchmark与性能规模，R05运维/支持矩阵/文档/许可证正文，R06版本签名/晋级/离线启动/发行清单。每包须固定制品digest、全部适用验收、独立审阅及核验才能完成。目前0/6内部包正式验收；开发工具扫描或文档存在只记录包内进度。

外部X01真实负责人/CODEOWNERS与安全联系确认；X02客户授权/数据分类/驻留及私有holdout；X03受保护CI/签名晋级与环境权限；X04真实runsc部署环境/证明；X05真实客户UAT与适用外部客户端。分别记录待输入、已提供、已验证、失败及影响的R/I项，不纳入开发分母，不据此阻塞无关公共模块，也不能豁免对应发行验收。

## 限定审阅与配方复用

审阅卡在执行前冻结接口、固定SHA、必要正例/拒绝/预算/取消/恢复边界。优先复用已批准oracle；独立审阅至少保留自己构造的决定性输入，避免只重复作者结果。只有新差异、新失败或明确未覆盖风险才能扩大验证；记录原因与范围，不反复跑完整矩阵或扩展无关攻击。

复用的是固定版本脚本与结构，不是旧运行结果。每任务复制/参数化已审阅配方到自己的证据目录，记录origin commit/path/SHA256和改动；必须使用UUID私有cargo/target/network/CA/home/tmp/PG，不能复用活跃容器、Secret、cache卷或完成证据。每次保存本次实际命令/退出/镜像/源码/fixture/hash/cleanup；REPORT/FINDINGS/HANDOFF采用相同结构：范围与verdict、固定来源、preedit smoke、决定性验证、保留失败、限制、复现、cleanup。

优先来源：a204 b2b3adab的pipeline无PG preoverlay/overlay-3脚本及报告；a200 a587031c的gateway/privateclang/TLS PG配方；a199 5b8cce62的CSV fresh oracle与SafeRust hash工具。配方使用前审读全文，不能盲跑携带旧UUID/路径的脚本。依赖图/工具链/目标改变才重建配方；同任务保持一个builder。源码与manifest单writer规则优先于复用便利。

## 扩展批次 E1-D（派工前冻结，不改变M1-D分母）

| ID / Plan归属 | 输入→输出 | 验收 | 状态 |
|---|---|---|---|
| B-D01 / SP12-W03 | typed expected/actual→exact comparison report | compiled CLI、decimal/null/multiset/partial/completion正确拒绝与接受、固定独立审阅 | 未验收，a212 |
| O-D01 / SP07-W05 | 两个真实draft artifacts→结构/审核差异 | 实际草案编译器fixture→compiled diff CLI，身份/类型/FK/unknown/strict拒绝 | 未验收，a213 |
| O-D02 / SP07-W06 | 真实role-pack command→packbytes+sealed bundle | 实际编译器双角色、独立hash、new-only/失败清理、compiled CLI | 未验收，a214 |

E1-D开发0/3，M1-D现17/24；两批已冻结开发合计17/27（62.96%），只覆盖这两批，不是全项目比例。a211是M1-I02的CLI链证据工作，不增加开发单位。

## 扩展批次 E2-D（本次派工前冻结）

| ID / Plan归属 | 输入→输出 | 决定性验收 | 状态 |
|---|---|---|---|
| M1-D18A / SP07-W05 | sealed reviewed draft+明确绑定分类→effective-oos | 实际两角色Pack消费、unknown/stale拒绝；不替代D18发布 | 未验收a217 |
| M1-D14A / SP06-W01 | 一角色多表完整policy set→原子列ACL | 真实双表查询/隐藏42501/全set replay/坏第二表整体回滚 | 未验收a218 |
| M1-D06A / SP04-W02 | 目标映射+generation→显式ANALYZE已知估计 | realimport unknown→known→query/source零写；不替代publication | 未验收a219 |
| F-D02 / SP05-W02 | realworker request→bounded parent verified IPC receipt | actualchild类型/lineage/EOF/nonzero/timeout kill+reap | 未验收a220 |
| B-D02 / SP12-W03 | actualquery-pg流→typed比较器输入 | actualquery→adapter→comparator/缺completion拒绝 | 未验收a222 |
| M1-D05A / SP04-W05 | operator targetconfig→实际installer CLI readiness | compiled init→import/replay/TLS/权限/partialschema拒绝 | 未验收a223 |

E2-D 0/6；M1-D仍17/24、E1-D0/3，已冻结开发合计17/33（51.52%），覆盖三个批次，不称全项目。a221是不覆盖竞态修复，不新增单位。a203/a211真实集成结果确认缺effective-oos与多表ACL/stats seam，M1-I01/I02标“已运行、接口待补”，保持未验收，不以退出0签完整链。

## E3-D（本次派工前冻结）

B-D03 / SP12-W03：成对完整question试次向量→精确pass^k与可复现成对question bootstrap报告；验收为手算k3/固定随机向量/独立小样本枚举/混合版本和缺试次拒绝、reannotation不移出分母、实际CLI与固定独立审阅。E3-D0/1，a225；已有三批不变。全部已冻结批次合计17/34（50%），不称全项目完成率；B-D03不是完整题库或客户benchmark。

## E4-D（派工前冻结）

M1-D14B / SP06-W01：真实sealed reviewed-effective artifact+administrative role/mapping bindings→已编译ColumnAclPolicy完整set；验收实际编译器输出消费、currentdecision/schema/fact/PII/semantic闭包与tamper/unknown/stale拒绝。E4-D0/1，a227；纯adapter不证明身份/授权安装/发布。所有已冻结批次合计17/35（48.57%），仍不是全项目率。a226是既有I01/I02组合证据，不能新增开发分母或自动签收I节点。

## H0-D：继承既有冻结任务卡的文件子项

F-D01 / SP05-W02：a207在dbaad666派工前已冻结的真实file-parser child→CSV/TSV/JSONL framed IPC与clean-EOF receipt，a224固定6bd24558独立限定批准。此处补充计数ID归属，不改变已派卡范围、不把测试拆新功能。H0-D1/1；parent/loader/公共隔离仍未验。历史已冻结卡未在初版M1表中列入，单列继承批次避免冒充M1进度。

## E5-D（派工前冻结）

B-D04 / SP12-W02：explicit synthetic suite+trusted executable hashes→实际query/adapter/comparator逐题独立试次报告；验收freshprivateTLS双角色/手写gold/hidden/timeout/缺completion/匹配截断前缀仍拒绝、保留所有失败trial与子进程回收。E5-D0/1，a231；只trustedSQL基线，不替代真实agent会话/历史冻结DB证明/客户benchmark。合计已冻结批次18/37（48.65%），不是全项目完成率。

Current coordinator acceptance update: a228 fixed9daeaa05 bridge APPROVE_LIMITED, 239 SHA256 entries verified0 plus smoke/test/Clippy0 and owncontainers absent; accept M1-D18A only. a230 fixed613aa981 stats APPROVE_LIMITED,175 SHA256 entries verified0, live/catalog/Clippy0 and owncontainers absent; accept M1-D06A only. E2-D2/6, M1-D17/24 unchanged; historical accepted43→45, unique authored45→46 with a227baf70c5c pending independent review; first-component review36→38, rework10/38=26.32%. Frozen aggregate20/37=54.05% before new E6. Integration0/4 and release0/6 unchanged.

- [x] M1-D18A: sealed reviewed draft + explicit bound classification -> effective-oos (a228 limited independent acceptance).
- [x] M1-D06A: mapped generation -> explicit ANALYZE estimate (a230 limited independent acceptance).
- [ ] M1-D14C / E6-D1 unit frozen BEFORE coding: trusted reviewed-effective command + administrative binding -> bounded compiled reviewed-policy CLI deterministic policy-set DTO, actual producer/CLI/compile-set tests and refusal exits. a234 owns new bin/test/contract only; E6-D0/1, aggregate20/38=52.63%, not global development rate. Semantic publication, auth, live installation and audit WAL remain separate.

Coordinator verified a232 init-pg fixed6e1e6e57 full reports,223 SHA256 entries0bad, smoke/proof/test/Clippy0 and owncontainers absent: accept M1-D05A only. Historicalaccepted45→46, firstcomponentreviews38→39, rework10/39=25.64%, authored46 unchanged. E2-D3/6; M1-D17/24 unchanged; frozen21/38=55.26%. a215 c27d6349 parser reap repair182hash0/fresh5testsClippy0 accepted repair only, no newunit/component; M1-D23 kernel remainsopen. a226559hash0 actualcomposition recorded, I01/I02 not accepted due missing operator mapping grants/publication/auth/NL/harness.
- [x] M1-D05A: init-pg real install/replay/import operator command, a232 limited independent approval.
- [ ] M1-D14D / E7-D: strict operator policy-set request -> actual aggregate ACL install CLI; a235 owns newbin/test/contract only, privateTLS2tables2roles and refusal/replay/query.
- [ ] M1-D06B / E7-D: explicit compiled analyze-pg operator command -> mapped stats receipt; a236 owns newbin/test/contract only, realimport/stats/catalog query.
E7-D0/2 frozen BEFORE code, aggregate21/40=52.50%; not global development rate. Integration0/4 and release0/6 unchanged. IPCparenta237 and correctedPackbundlea238 independent fixed reviews queued, no additional countableunits.

Coordinator accepts M1-D14B limited sealed adapter on baf70c5c after a233 full reports/148hash0/smoke21+independent9/Clippy0/owncontainers absent. E4-D1/1, frozen22/40=55%; historical46→47, firstcomponent39→40/rework10/40=25%; authored46→47 with a234bcc86734 CLI262hash0/smoke21+CLI6/Clippy0 pendinga239fixedreview. M1-D17/24, integration0/4 release0/6 unchanged.
- [x] M1-D14B: reviewed effective artifact+bound decisions->compiled policy sets (a233 limited independent review).
- [ ] M1-D14E / E8-D0/1 frozen BEFORE code: explicit administrative mapping reader provisioning->actual catalog/query without fixture mapping GRANT; a240 module+libexport/newtest/contract only, no existing installer or migration edits. Does not authenticate or guarantee tenant isolation; metadata exposure named. Aggregate22/41=53.66%, not global completion.

Coordinator verified full independent reports and hashes: a229202entries0bad/realpostgrant rollback+query/Clippy0, a237257entries0bad/fresh5+author8/Clippy0, a238104entries0bad/actualcompilebundle+concurrentnooverwrite/Clippy0; allowncontainers absent. Accept M1-D14A, F-D02, O-D02 within frozen scope. Historicalaccepted47→50; firstcomponentreviews40→42 (ACLextensionexcluded), rework10/42=23.81%. E2-D5/6, E1-D1/3, E4-D1/1; aggregate25/41=60.98%, not global project rate. M1-D17/24, integration0/4/release0/6 unchanged.
- [x] M1-D14A: aggregate two-table ACL install/replay and atomic failure rollback (a229).
- [x] F-D02: real IPC parent parse/cleanEOF/recovery, kernelUnsupported preserved (a237).
- [x] O-D02: rolePack bundle exact bytes and non-replacing hardlink (a238).

Coordinator accepts M1-D14C limited reviewed-policy CLI bcc86734 after a239fullreports/153hash0/smoke6+independent5/Clippy0/owncontainers absent. E6-D1/1, historical50→51, firstreviews42→43/rework10/43=23.26%; aggregate26/41=63.41%. a231runner358hash0/focused7+liveproof0 and a235ACLCLI170hash0/12testsClippy0/owncontainers absent, authoredcomponents47→49 pendingindependentreviews. M1-D17/24 integration0/4 release0/6 unchanged.
- [x] M1-D14C: compiled reviewed-policy CLI two-role sealed producer consumption (a239).
- [ ] F-D03 / E9-D0/1 frozen BEFORE code: actual IPC parent backed parse-file CLI for CSV/TSV/JSONL bounded summary/cleanEOF; a245 ownsnewbin/test/contract only. Aggregate26/42=61.90%, not global project rate.

Coordinator accepts frozen M1-D14D ACL CLI13566602 aftera241fullreports/253hash0/smoke12+actualindependentquery/Clippy0/owncontainersabsent; fixturemapping/ANALYZE separate. E7-D1/2, accepted51→52, firstreviews43→44/rework10/44=22.73%. a2401f1028f6 mappingmodule253hash0/live+Clippy0 and a2366ac22165 statsCLI216hash0/livecatalog+Clippy0/owncontainersabsent authored49→51 pendingfixedreview. Aggregate27/42=64.29%, M1-D17/24 integration0/4 release0/6 unchanged.
- [x] M1-D14D: real aggregate install-column-acl CLI and independent compiledquery (a241).
- [ ] M1-D14F / E10-D0/1 frozen BEFORE code: bounded provision-mapping-reader CLI -> actual mapping provision receipt/catalogquery; a249newbin/test/contract only. Aggregate27/43=62.79%, not wholeproject rate. a248 fixedoperatorcomposition evidenceonly, no newdevelopmentunit.

Coordinator accepts B-D01/B-D02/B-D03 aftera242fullreports/459hash0/44freshCLIcases+actualTLSchain/all3Clippy0/owncontainersabsent; truncationprefix canmatch butrunnerseparatelymustrefuse. Accept O-D01 aftera243fullreports/186hash0/fresh5+author9/Clippy0/owncontainersabsent. E1-D3/3,E2-D6/6,E3-D1/1, historical52→56, firstreviews44→48/rework10/48=20.83%, authored51unchanged. Frozen31/43=72.09%; M1-D17/24 integration0/4 release0/6 unchanged.
- [x] B-D01: exact typed result comparator (a242).
- [x] B-D02: actual query-result adapter, truncation boundary retained (a242).
- [x] B-D03: full-vector pass^k and paired small bootstrap oracle (a242).
- [x] O-D01: sealed artifact diff with stale review and curated preservation (a243).
- [ ] B-D05 / E11-D0/1 frozen BEFORE code: actual runner reports ->paired trial-statistics input, preserve planned failures/reannotation, stricttuple/fullvector; a250standalonewriter. Aggregate31/44=70.45%, not global project completion.

Coordinator accepts frozen M1-D14E mappingreader aftera246fullreports/186hash0/freshactualcatalogquery+Clippy0/owncontainersabsent, M1-D06B analyzeCLI aftera247257hash0/realcompiledinitimportanalyze/catalogquery+Clippy0/owncontainersabsent, B-D04 trustedSQLrunner aftera244391hash0/livepairedroles/k2/truncatedprefixfail/Clippy0/owncontainersabsent. Metadata wholemappingtable exposure and backendcleanupunknown retained. Historical56→59, firstcomponent48→51/rework10/51=19.61%, authored51unchanged pendinga245/a249verification. E8-D1/1,E7-D2/2,E5-D1/1; aggregate34/44=77.27%, not wholeproject rate. M1-D17/24 integration0/4 release0/6 unchanged.
- [x] M1-D14E: explicit mapping reader provisioning and catalogquery (a246).
- [x] M1-D06B: real analyze-pg operator CLI (a247).
- [x] B-D04: trustedSQLrunner exact/fail/refusal/timeout/truncation with full trial denominators (a244).

S5-D01 / E12-D0/1 frozen BEFORE code: explicit admitted sealed tablecards -> structureddescriptionrequest -> completecapturedresponse -> review-only generated label/description/grain/caveats/open_questions candidates. a253 ownsbuildernewmodule/libexport/newtest/contract only; no modelnetwork/credentials. Actual providertransport/residency/cost and fullS5 remain futuregates. Aggregate34/45=75.56%, not global rate. a245/a249 author fullreportsread and evidencehash verified; same-session nested a249 review is not independent approval.
- [ ] S5-D01: bounded evidence-bound generated description candidate protocol.

Actuala248 composition refused confirmed int8 quantity in reviewed-policy: noDTO/install/mapping/analyze/query executed; I01I02 stayunaccepted. Mandatory productcompatibility extension a254 M1-D14G / E13-D0/1 frozen BEFORE code, integerquantity int2/int4/int8 only with same sealeddecision/PII controls; neverreclassify/hideamount to pass. Check targetmappinggeneration distinctsourcegeneration in composition. Aggregate34/46=73.91%, notglobalrate. a2508e7925d7 authored actualrunnerreports->adapter->statistics16testsClippy0 pendinga255independentreview; no qualitywinfromallfail structuralgate.
- [ ] M1-D14G: reviewed confirmed integer quantity to actual typed policy/query.

B-D06 / E14-D0/1 frozen BEFORE code: publicsynthetic versioned questionbank validator->contentboundmanifest/separateagent-scorerview, unresolvedgold staysreannotation; a256standalonewriter only. No privateholdout/customer/public300/humanannotation claim. Aggregate34/47=72.34%, not globalrate. a248338hash0 provessemanticrefusalnotquerysuccess; a250340hash0/16testsClippy0/owncontainersabsent authored53→54 pendinga255review. No accepted/rework change.
- [ ] B-D06: public question-bank manifest and gold-separated views.

Coordinator accepts F-D03 parse-file CLI aftera251 fullreports/188hash0/smoke12+fresh12/Clippy0/owncontainersabsent; actualIPCcleanEOF summary, import/published/pg_run_completefalse/kernelUnsupported. Accept M1-D14F mappingreaderCLI aftera252 fullreports/167hash0/privateTLS14testsClippy0/owncontainersabsent, same-sessiona249nestedreviewexcluded. E9-D1/1,E10-D1/1, historical59→61, firstcomponent51→53/rework10/53=18.87%, authored54unchanged. Aggregate36/47=76.60%, notwholeproject rate; M1-D17/24 integration0/4 release0/6 unchanged.
- [x] F-D03: actual parse-file IPC parent CLI bounded summary (a251).
- [x] M1-D14F: explicit provision-mapping-reader CLI (a252).

Coordinator accepts B-D05 trialreportadapter aftera255 fullreports/273hash0/16sealedtests+independentactualrunnerfullvectors/Clippy0/owncontainersabsent. E11-D1/1, historical61→62, firstreviews53→54/rework10/54=18.52%. a253fbc10b04 S5candidate351hash0/proof7 2lib+10testsClippy0/owncontainersabsent authored54→55 pendinga257review; failedproofs1–6 retained. Aggregate37/47=78.72%, not globalrate.
- [x] B-D05: actualrunner reports ->pairedfulltrialvectors (a255).
- [ ] S5-D02 / E15-D0/1 frozen BEFORE code: compiled description-candidate CLI prepare/complete publicS5 protocol, a258ownsnewbin/test/contractonly, capturedresponse noactualmodelclaim. Aggregate37/48=77.08%; M1-D17/24 integration0/4 release0/6 unchanged.

a254497hash0/postedit21+6+6testsClippy0/livequantitychain0/owncontainersabsent verifiedauthorevidence; actualpolicygeneration mappingrun/sourcecut48 distinct targetfence1. Priorcoordinator suggestiontoforce targetfencegeneration was incorrect; preserve correctactualmapping contract. Pendinga259 independentM1-D14G review, noacceptance/newcomponentcount. a256c275aa63 questionbank511hash0/13testsClippy0/owncontainersabsent authored55→56 pendinga260review.
- [ ] M1-D25 / E16-D0/1 frozen BEFORE code: Rusttwo-phaseinspect/apply-reviewed onboarding->actualcompiledPGCLIchain/currentmanualreview/privateTLS2tables2roles/quantity and hiddencontrols; a261standalone tools/pg-onboarding only. Ready_for_trusted_query notsemanticpublish/auth/NL/harness. Aggregate37/49=75.51%, notglobalrate; M1-D17/24 integration0/4 release0/6 unchanged.

Coordinator verified fulla257REPORT/FINDINGS/HANDOFF/165hash0: actualproductcontractfailure selected_datasets sorting, proof2 independent6pass1fail/docker101, Clippy0; same-key authorfixtures hadmissedphysicalA->q_41ordering. Firstcomponentreviews54→55/rework10→11, rate11/55=20.00%. S5-D01 unaccepted,a262repaironlyno newunit/component. a258827e768b CLI119hash0/preedit2+10/proof5Clippy0/owncontainersabsent authored56→57 pending correctedlibrary+CLI fixedreview aftera262. No newacceptance: historical62/frozen37/49/M1-D17/24/integration0/4/release0/6 unchanged.

Coordinator accepts M1-D14G integerquantity aftera259fullreports/hashverified/freshphysicalint2int4int8producer+realint8query/Clippy0/owncleanup; existingadapterextension notnewcomponentfirstreview. Accept B-D06 publicsyntheticbank aftera260fullreports/hashverified/freshhandbank+separatedcanaries/Clippy0/owncleanup. E13-D1/1,E14-D1/1; historical62→64, firstreviews55→56/rework11/56=19.64%. OriginalM1-D17/24 integration0/4 release0/6 unchanged.
- [x] M1-D14G: confirmed integer quantity remainsquantity throughpolicy/query (a259).
- [x] B-D06: publicsyntheticbank manifest and separateagent/scorerview (a260); full300/realhumanannotation/customerholdoutstillpending.
- [ ] S5-D03 / E17-D0/1 frozen BEFORE code: actual HTTPS modelprovider adapter/privateTLSproof->capturedresponse/S5candidate boundedfailure and usage; a263standalonetools/description-provider only, no realkeys/customercalls. Aggregate39/50=78%, notglobalrate.


2026-10-06 coordinator: ONE20minute unified3handles observation: a26254137 exited0, a26132052/a26313071 live. Retire a262handle. Fulla262REPORT/FINDINGS/HANDOFF read; 248hashentries0bad; redtest101 preserved, proof/CLI/tests/Clippy0 and owncontainers absent. Source56b79136 fixes dataset order without changing cards/candidates. S5-D01/S5-D02 remain unchecked until independent combined fixed review a264; no counts from repair.

Frozen BEFORE new development, three independent owned paths:
- [ ] RT-D01 / E18-D0/1: tools/session-transition Rust deterministic session/turn reducer and CLI, identity tuple/CAS/event replay/time/tool budgets. Durable database/authentication/actual physical release remain later gates.
- [ ] RT-D02 / E19-D0/1: tools/answer-citations actual query-pg receipt-bound answer citation validation; local operator binding explicitly declared, no authenticated audit or answer-entailment claim.
- [ ] B-D07 / E20-D0/1: benchmarks/question-bank-suite validated public bank -> role-specific exact sql-runner suite/files; unsupported kinds refuse whole compilation, reannotation denominator preserved.

Development frozen39/53=73.58%, authored57/historicalaccepted64/firstreviews56/reworked11 (19.64%) unchanged. This fraction covers frozen children only, not wholeproject. M1-D17/24, integration0/4, internalrelease0/6, externalX01-X05 separately unchanged. Development cap12, reviews outsidecap, global10builders. New task writes only its isolated standalone directory and UUID evidence; no rootmanifest/lock or existing product files. Coordinator wait ONE20minutes then ONE unified actualhandle observation; no intermediate polling.


ONE20minute unified6handles: a26132052 exited0, a26313071/a26453702/a26588746/a26624070/a26775098 live; no repeatedpolling. Fulla261reports read and596hashentries0bad; preedit smoke-attempt2/postedit4Clippy/livechain2 actual0. Authoredcomponents57→58, acceptanceunchanged. Actual two-role quantity/hidden/Pack/query ready_for_trusted_query, not I01/I02/fullsemanticpublication/auth/NL. Concrete a261 residuals require minimumrepair before fixedreview: failed database_state unchanged_by_this_tool after appliedchild steps and duplicate JSON lastwins. a268 repair owns only pg-onboarding standalone +UUID evidence, no newunit/denominator/firstreview. Frozen39/53, historical64, firstreviews56/reworked11 unchanged. a261 handle retired; actualdevelopment back to5 plus1independentreview after repair launch. NextUUIDa269. ONE20minute wait thenONE unified6actualhandles; no intermediatepolling.


Coordinator accepts S5-D01 repaired description protocol and S5-D02 compiled CLI after full a264 reports/187hashentries0bad/fresh physicalA-b publicAPI+actualCLI ten documents/focused18tests/Clippy0/owncontainersabsent. Fixed567d621e contains56b79136 repair and827e768b CLI. a257 REQUEST_CHANGES remains on its original tree, revieworacle proof1hashcomparison error preserved/proof2 decisive0. E12-D1/1,E15-D1/1; historical64→66, firstunique reviews56→57 (S5-D01 alreadycounted), reworked11/57=19.30%, authored58 unchanged. Two accepted childunits only, fullS5/provider notaccepted.
- [x] S5-D01: corrected review-only description candidate protocol (a264).
- [x] S5-D02: actual description-candidate prepare/complete CLI (a264).

Frozen BEFORE coding two independent businesssemantic mainline modules:
- [ ] SEM-D01 / E21-D0/1: actual core OOS business calendar -> as_of-bound civil anchors/phrase intervals/businessTZ/DST and explicit closure; a269 tools/business-calendar only.
- [ ] SEM-D02 / E22-D0/1: actual core OOS temporal enum -> historical/current classes and exact observed coverage/warnings, a270 tools/enum-history only.

Frozen development41/55=74.55% (localfrozenchildren only, notglobalproject rate). M1-D17/24/integration0/4/internalrelease0/6/externalgates unchanged. ONE20minute unified6handles: a26313071/a26453702 terminal; a26588746/a26624070/a26775098/a26858054 live. a263 lacks full reports, resume SAMEUUID originalcard actual33224 firstcollectalreadyrunningproof results/no duplicatebuilder. No provider acceptance fromexit0. NextUUIDa271. Max12development, reviewsoutside, global10builders; after two newlaunches7development0reviews at lastactualobservation. ONE20minute wait thenONE unified7actualhandles.


ONE20minute unified7handles: a26333224/a26588746/a26624070/a26858054 exited0; a26775098/a26969958/a27016428 live. Retiredterminalhandles. Fullreportsread andinventories a263387/a265190/a266244/a268181 entries all0bad; owncontainersabsent. Provider25testsClippy0/rootregistry248missing0mismatch0, session15testsClippy0, citation10testsClippy0, repair24testsClippy0(redactualfaultretained). Same-session nestedreviews notindependentapproval. Authored58→61 (provider/reducer/citations), accepted66/first57/reworked11 unchanged. a271–a274 fixedindependentreviews dispatched separately, reviewsoutside12devcap, onlyfrozenboundarysuite.
- [ ] RT-D03 / E23-D0/1 frozen BEFORE code: tools/session-store actualPG durable rowlocked reducer/CAS/eventreplay and precise commituncertainty; a275owned standalone+SQL only. Reducera265pendinga272review, fixeddependencychangesrequireexplicitnewproof/no moving source.

Developmentfrozen41/56=73.21% localchildrenonly. M1-D17/24/integration0/4/internalrelease0/6/externalconditions unchanged. Existingactual3development plusnewsessionstore=4development and4review, max8builders/10. NextUUIDa276; ONE20minute wait thenONE unified8actualhandles, no intermediate/repeatedpolling.


ONE20minute unified8handles: a26775098/a27016428 exited0; a26969958/a27171878/a27283678/a27391440/a27417848/a27566011 live. No repeatedpolling, retiredterminalhandles. Fulla267reports413hash0/18testsClippy0 and a270reports276hash0/23testsClippy0/owncontainersabsent verified; authored61→63, acceptance/firstreviews unchanged pendingindependentfixedreview. a267actualnot_found emptyrows mismatch retained as real interfacegap, notaccepted productquality.
- [ ] B-D08 / E24-D0/1 frozen BEFORE code: explicit compatible empty_result opt-in connects adapter/runner/questionbank-suite without changing default emptytables or gold; actualzeroquery/notfound comparison, a276 owns onlythreebenchmarkworkspaces minimalextension.
- [ ] S5-D04 / E25-D0/1 frozen BEFORE code: actual verifieddescriptioncandidate -> explicit anchored approve/reject/defer -> corevalid curateddataset descriptivefields with CAS/stalequeue/otherfields preserved, a277tools/description-review only.

Frozen41/58=70.69% localfrozenchildren only; historical66, first57/reworked11=19.30%, M1-D17/24/integration0/4/internalrelease0/6 unchanged. Current4review slotsfull so enum/banksuite independentreviewsqueued, never use same-session reviews asacceptance. Withnewlaunches5development+4review/max9Rustbuilders<10. NextUUIDa278; ONE20minute wait thenONE unified9actualhandles, no intermediate/repeatedobservation.


Coordinator accepts four frozen development units after full independent reports/inventory/rawexit: S5-D03 a271195hash0/25author+7freshTLS testsClippy0; RT-D01 a272105hash0/15author+11fresh testsClippy0 plus separate corrected11registrytriplecheck0 (proofcontainer2 preservedcheckerfailure); RT-D02 a273191hash0/10author+9freshCLI testsClippy0; M1-D25 a274407hash0/24focusedClippy0+freshprivateTLSchain0 and honestpostspawn/strictJSON repair. Owncontainersabsent for allfour. Historical66→70, firstunique57→61/reworked11/61=18.03%, authored63unchanged. E17/E18/E19/E16 all1/1; frozen45/58=77.59%. FullS5/auth/durableCAS/productionaudit/semanticpublication/NL/harness/I01/I02 remainopen.
- [x] S5-D03: actual HTTPS provider -> review-only candidate, explicitdeclaredcost/residency (a271).
- [x] RT-D01: session/turn reducer + CLI, durableCAS separate (a272).
- [x] RT-D02: localreceipt-bound citation validation, authenticatedaudit/entailment separate (a273).
- [x] M1-D25: actualtwo-phasePG onboarding + repairedfailuretruth/strictJSON (a274).

Correct process accounting: prior note said9handles/5development+4review, but after a267/a270 closed only a269/a275 plus a276/a277 remained =4development+4reviews=8handles. Observed all8 exactlyonce: a27171878/a27283678/a27391440/a27417848 exited0; a26969958/a27566011/a27676818/a27719446 live. No9thprocess launched/nohiddenpoll. Retiredfourterminalhandles. a278 independentenumreview new1builder0PG.
- [ ] H-D01 / E26-D0/1 frozen BEFORE code: P9 actualreleasedPi RPC stdio probe/Rustdriver/version/license/integrity; a279testing/spikes/p09-pi-rpc only, no fabricatedprotocol/no realkeys. Actual launch/send/events/abort/transcript/restore required beforeapproval; unsupportedstaysunaccepted.

Frozen45/59=76.27% localchildrenonly, M1-D17/24/integration0/4/internalrelease0/6/externalgates unchanged. NewPi probe gives5development+1review/max6Rustbuilders<10/devcap12. NextUUIDa280. ONE20minute wait thenONE unified6actualhandles, no intermediate/repeatedpolling.


ONE20minute unified6actualhandles: a26969958/a27566011/a27676818/a27719446/a27886321/a27998869 allconfirmedlive. No interim/repeatedpoll, no restart, no acceptance change. Previousgoalturn was progress (fourfixedunitacceptances/newrealPi probe); thisobservation is verifiedwait. Continue independent CDCmainline with isolatedownedpath.
- [ ] CDC-D01 / E27-D0/1 frozen BEFORE code: actual bounded Rust normalizedPGtransaction assembler, BEGIN/DATA/END/rollback/count/hash/commitLSN and read/seen/complete vs NOTapplied; a280tools/cdc-transaction only. ActualDebezium/P2P11/fencedtargetwriter/offsetACK/durablelarge-spool remainlater requiredwork, no made-uptransactional guarantee.

Frozen45/60=75% localchildrenonly; authored63/historical70/first61/reworked11=18.03%, M1-D17/24/integration0/4/internalrelease0/6 unchanged. WithnewCDCassembler6development+1review/max7builders<10/devcap12. NextUUIDa281. ONE20minute wait thenONE unified7actualhandles, no intermediate/repeatedpolling.


ONE20minute unified7handles: a27566011/a27676818/a27719446/a27886321 terminal0; a26969958/a27998869/a28028070 confirmedlive. Retireclosedhandles. Fulla278reports165hash0/23author+11freshindependenttestsClippy0/ALL115registrysubset0/owncontainersabsent -> acceptSEM-D02 only. E22-D1/1, historical70→71, firstunique61→62/reworked11/62=17.74%, frozen46/60=76.67%. M1-D17/24/integration0/4/internalrelease0/6 unchanged.
- [x] SEM-D02: temporal enum history/current classes/exact observedcoverage, DBsweeper/oo_dict/auth/metriccert separate (a278).

Fulla275 reports273hash0/11lib+12actualPGtestsClippy0/owncontainersabsent authored63→64 pendinga281 fixedindependentreview. Concrete authorprocessdeviation preserved: standalone recipe cargo generate-lockfile followed by cargo update --precise despite no-updatecard; finalclaimedrootregistry subset must independentcheck ALLtriples. a281 usesCOMMITTEDlock only, no generate/update; don'thidehistoricalfailure/newtransitiveattempt. No acceptancebeforeindependentproof.

a276/a277 terminal0 lackfullreports, resume SAMEUUID originalcards firstcollectexistingownactualbuilder results/no duplicatebuilder/PG. No acceptance fromexit. NextUUIDa282. Afterresumescurrent5development+1review/max6builders<10/devcap12. ONE20minute wait thenONE unified6actualhandles no interim/repeatedpolling.


ONE20minute unified6handles: a27998869/a27629817/a27743717 terminal0; a26969958/a28028070/a28145957 live. Retireclosedhandles. a276/a277 stillmissingfullreports, resumeSAMEUUID firstcollectexistingproof/fixactualfailure/no duplicatedbuilders; noexitonlyacceptance.
Fulla279reportsread;473manifestentries0bad bycoordinator. ActualPi1.0.4 launch/send/textdelta/abort/get_messages/sessionfile/switch_sessionrestore and12boundarytests0/live0, butClippy127 andhostshasum inventory contrarySafeRustoracle. Prototype16cd808d NOTaccepted/authoredcountpendingverification. a282minimalClippy+Rustinventoryrepair preserving originalevidence. Pinunder7dayfloor remainsprototype no rootbaselinepromotion.
Resource clarification: cap12 CONCURRENTdevelopmentGrokPROCESSES excludingreviews; global10 CONCURRENT Rustbuilders/max1pertask, NOT lifetimecontainers. a27915sequentialcontainers didNOTbreachusercap; itsmisinterpretation causedprematurevalidationstop. No newunit/count. Metricsunchanged frozen46/60/historical71/authored64/first62/reworked11=17.74%, M1-D17/24/integration0/4/release0/6. Afterresumes+repair5development+1review/max6builders. NextUUIDa283. ONE20minute wait thenONE unified6actualhandles.


ONE20minute unified6actualhandles: a26969958/a28145957/a27689379/a27715721 terminal0; a28028070/a28288449 confirmedlive. Retireclosedhandles. Fulla281reports217hash0/23author+8freshPG testsClippy0/ALL128registrysubset0/owncontainersabsent -> acceptRT-D03 only. Committedlock reviewed no update/generate; historicalauthorpreciseupdate deviation remains. Reconcile isevent-id presence notfingerprintcertificate; conflicting transition stillrefuses no updates. E23-D1/1 historical71→72/firstunique62→63/reworked11/63=17.46%, frozen47/60=78.33%. M1-D17/24/integration0/4/internalrelease0/6 unchanged.
- [x] RT-D03: actualPG durable rowlocked sessionreducer/CAS/idempotency/commit_unknown recovery (a281); publicauth/physicalrelease remainopen.

Fulla269 reports273hash0/21testsClippy0/ALL115rootregistry0/owncontainersabsent authored64→65 pendinga283 independentcalendarreview ec8b3953. a276/a277 lackreports despite resumedactor terminal0, resume SAMEUUID firstexistingresult/no duplicatebuilder; noauthored/acceptance count yet. a283newreview1builder0PG. Afterresumes4development+1review/max5builders<10/devcap12. NextUUIDa284; ONE20minute wait thenONE unified5actualhandles, no intermediate/repeatedpolling.
