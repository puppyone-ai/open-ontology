# 冻结验收计数与复用规范（OO-COUNT-001，2026-10-06）

本文件是 Plan 的执行附录。冻结当前优先纵向批次 M1-D 的24个开发验收单位；全项目87个父工作项和16个Phase仍全部保留。**16/24（66.67%）只表示此批次开发验收，不表示全项目开发完成率或可发行。** 后续数据库、文件、CDC等按单独冻结批次报告，不能纳入本批次分母后回填成功。

## 计数规则

开发单位完成要求：固定接口与范围、真实实现、Linux/MSRV局部测试与必要边界测试、固定提交独立批准、协调者核验原始退出及证据hash。已批准的有限接口范围按原review约束计数；发现强制返工时增加缺陷状态，不删除历史批准、不增加新功能单位；当前可用性另标受影响。修复、补报告、增加测试和重审均不增加分母或交付功能数。16个已验单位可包含同一功能的API与CLI，因接口/用户输出及验收不同；不能据此宣称16个独立产品组件。

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
| M1-D17 | 实际PG查询CLI | trusted SQL→typed rows/audit/exit | 未验收，pending:a208 |
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

当前集成0/4；该0只表示四条完整链尚未验收，不否定16项开发交付。部分运行记录可标“已运行、待修复/待审”，不能冒充链完成。

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

E1-D开发0/3，M1-D仍16/24；两批已冻结开发合计16/27（59.26%），只覆盖这两批，不是全项目比例。a211是M1-I02的CLI链证据工作，不增加开发单位。
