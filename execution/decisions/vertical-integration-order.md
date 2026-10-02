# PostgreSQL 真实纵向闭环：下一批整合与验收顺序

状态：执行顺序冻结；所列未交付或未复审能力仍为 pending。该文件不替代完整 Spec/Plan/87 工作项。

## 最新调度（2026-10-02；覆盖下方旧状态）

- main源role GUC修正34fae5d获a097终态有限approve；协调者核验SQL日志hash后发现独立oracle源码/最终完整输出未留存，原UUID补证中，源准入暂未签。旧报告保留。
- NFC3ed558a已真实交付且root释放，a094固定复审；结构closure eebc0ea获a086限定批准，完整草案未签。类型任务需与仍live的a093 connector writer串行交接。
- 目标schema8370498发现NULL分段绕过；a092只追加前向迁移及新契约，旧迁移hash冻结。
- SDK runtime bde2e8c、P5 IPC6831d6e已交付，a090/a091固定SHA只读复审。
- 基础Rust supervisor a082原UUID续接，基础门禁不阻塞上述独立产品修正。
- a095独立CLI query/JSON/Markdown/退出码实际启动，root唯一writer；a098类型真实解码已启动，connector代码独占、主依赖等待a095显式交接；reader和真实apply两张卡仍prepared。所有任务max2全局Rust builders，容量不足灵活等待；CLI退出即核验，不定30分钟、不持续轮巡。

## 当前状态补充（2026-10-02；旧运行记录保留）

- 源三HIGH修正4ecdbf7获a074限定批准，a083正在main源范围整合并独占root依赖；旧b2禁止准入，main最终固定提交仍待交接/复审。
- 控制schema70a4e72获a068批准，event绑定9224d31获a079批准；Rust jobs驱动、真实installer及完整W01仍未完成。
- 草案4c989bc经a071发现五项问题；a081只修结构closure，NFC依赖修正待root写权释放。
- 目标schema8370498交付，a085固定复审进行中；receipt-only不等于Loader导入。
- P5原型efb61d6经a076要求返工，a080独立Rust隔离纠正；SDK8efeb23由a084固定复审中。

下列任务卡已准备，**尚未派发、没有编码写权**：

1. [PG无损类型解码](../prompts/v01-pg-lossless-types-prepared.md)：源整合固定接口后，按root串行顺序与NFC任务协调。
2. [有界快照读取](../prompts/v01-pg-bounded-reader-prepared.md)：消费类型结果，实际Arrow stream和源backend退出证据。
3. [未发布目标apply](../prompts/v01-pg-target-apply-prepared.md)：消费reader及独立批准的target fence，实际数据/质量/receipt同事务。

每次派发补上真实基线/输入SHA、唯一UUID/evidence及writer范围；“prepared”不计交付、验收或运行中。只读review可与后续无冲突开发并行，未经批准的输入不得用于放行。

## 当前可用输入

| 输入 | 固定提交/状态 | 使用限制 |
|---|---|---|
| Core 类型、配置、OOS、规范值 | 主工作区已提交模型；a036批准修复切片 | 纯模型，不代表真实类型解码完成 |
| PG源入口/catalog | b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74 | a061发现三HIGH，禁止据旧admission启动导入；a066修正运行中 |
| 控制jobs schema | 70a4e72c3ea698038e0bdd38df29e38c01dfd24d | a068复审中；没有Rust队列驱动、目标fence或100worker验收 |
| 纯结构草案 | 4c989bce58407cf19d3d93dc600162d7322d92f8 | a071复审中；合成catalog，不代表真实PG导入、PII分类或发布 |
| 目标fence/receipt | a072运行中 | 待真实同连接锁、失锁接管、幂等冲突、原子事务证据与固定提交复审 |
| 查询parser隔离 | a065运行中 | 待固定交付/独立审阅；R4/OID/函数绑定与完整R1–R9仍另验 |

## A. 源修正接收与主工作区整合

1. a066真正退出后立即读取完整REPORT/HANDOFF、固定commit/tree/parent及实际失败/通过日志。检查三HIGH的全局可达写能力、连接身份与环境隔离、private-CA-only真实证据。保留未知与未测试条件。
2. 对修正固定SHA启动只读独立复审；不能先把移动vendor代码或作者13个旧测试通过当安全验收。复审须验证官方sqlx-core/postgres 0.9.0 tarball校验、完整license、精确补丁差异、feature graph及实测TLS/PGOPTIONS/非所选schema风险。
3. 两个path patch必须作为修改后的上游组件记录来源和补丁身份；版本仍叫0.9.0并不意味着与官方发布字节相同。检查新no-roots feature是否真的消除webpki数据依赖；即使运行时不加载数据，编译制品仍包含数据时其分发义务不能忽略。ISC策略审阅与剩余license正文工作独立保留。
4. 独立结果可接收后，由专用integration writer将源功能、必要补丁、contract与fixture按明确路径整合到main。基线4213134不得整体merge覆盖main的新xtask/builder/jobs。共享Cargo.toml/lock仅integration writer修改；不会重置工作区或覆盖其他任务未提交文件。
5. 使用main现有root manifest逐项合并所需依赖/features/path patch，再以真实cargo解析更新lock，记录新增/删除/变更包和patch来源。有限构建快照必须包含两个vendor的必要manifest/source/build输入/license/provenance，拒绝漏文件、symlink或漂移；不复制整个registry/cache。
6. 开始前Linux1.94 focused source基线检查；整合后source实测回归与必要架构检查。若未来快照过滤器或架构分类不支持vendor，单独记录具体失败并由授权writer修正，不能无限排除第三方代码以跳过安全检查。完整双架构矩阵留给闭环集成节点。

## B. 有界源读取与目标导入

分成可固定提交审阅的三项，不把DDL或catalog组件当数据导入。

| 子项 | 输入/接口 | 测试与交付 |
|---|---|---|
| PG只读读取/解码 | 已批准源admission、固定catalog/selection、同一个RR snapshot、显式type policy；仅批准类型和稳定键。未知/domain/enum/宽numeric等未经实现不能自动cast为近似类型 | 真PG行数据/NULL/大值/组合键/稀疏键；rows/bytes/backpressure预算，取消后后端已停/rollback确认；读取值与独立SQL oracle比对。交付versioned batch和能力限制 |
| 未发布目标apply | a072固定协议与已验证目标fence；同专属连接锁+generation+事务内数据/质量/receipt，所有值参数化，目标物理名称安全引用 | 真双库源零写；重复batch幂等、payload冲突拒绝、强杀前后恢复、不混用失效源快照、新owner接管旧owner不能提交。交付LOGGED未发布数据generation与耐久catalog/lineage，不给Agent列权限 |
| 真实catalog到草案 | 导入完成的固定catalog/schema/config/type/evidence digest、a071批准编译器 | 从真实PG fixture采集/导入到draft的可重放链路；独立核验每层hash、真实复合FK、恶意comment隔离、PII仍unknown，审核变更绑定版本；输出可审阅草案和完整证据 |

物理名称不可直接使用pgident/v1的完整hex逻辑id：需在真实PG验证63字节限制/截断碰撞，使用持久、可审阅、版本化的logical-to-physical映射或明确拒绝；禁止隐式截断。源snapshot失效且无全轮耐久spool时，废弃整个未发布run/shadow，以新generation从头读，不能拿旧已完成块拼接新快照。

## C. 审核、权限查询与CLI对话

审核是用户主体/owner授权的Control接口操作，不能因调用compile_structure_draft或填写accept字段就获得发布权限。先完成列分类、实际DB角色列ACL/RLS、审计与版本绑定；未经分类/未审核资产不授予Agent读取。

查询路径使用Gateway公开接口：a065 parser经独立审阅后仍需真实PG prepare/bind/portal、OID/函数/operator/cast绑定、只读事务、请求预算与可确认取消。沿用a056权限原型的真实SET/claims/隐藏行列攻击，但原型approve不替代生产Gateway验收。CLI通过SDK/HTTP调用同主体Gateway，不直连数据库、不拿管线或owner凭证；未配置模型明确报错，不用固定回复假装对话。

闭环集成节点冻结全部源码/依赖/迁移/fixture/模型配置，再运行适用双架构/MSRV矩阵、真实公开E2E和权限负例。实际负责人、客户私有holdout、protected CI、runsc与发行批准按各自门槛保留；缺证据只阻断相应验收，不阻断独立公共模块开发。

## 计量与完成勾选

每项真实交付立即本地明确路径提交，审阅绑定固定SHA；复审request_changes按同一功能返工，不重复增加功能交付数。To-do只有固定提交独立结果和协调者核验足够时勾选对应子项。组件通过不勾整个SP03/SP04/SP07/SP06/SP09，未覆盖的完整AT/原型与其余数据库/文件/CDC/harness持续推进。
