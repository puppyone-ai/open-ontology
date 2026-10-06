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
