# Open Ontology v0.1 开发执行 Plan

| 属性 | 值 |
|---|---|
| Plan ID / 版本 | OO-PLAN-001 / 1.0.1 |
| 状态 | Implementation in progress；实际任务状态以开发台账和固定提交证据为准 |
| 日期 | 2026-10-02，Asia/Shanghai |
| 规范基线 | [OO-SPEC-001 v1.0.1](open-ontology-v0.1-spec.md)；[19 份来源的哈希基线 revision 2](sources-baseline.json) |
| Spec SHA-256 | `accac91210579da06387c94a28a6e71430a9c629cab9c6921c137d2e2f72fb91` |
| 使用对象 | 工程师、编程 Agent、组件 reviewer、安全 reviewer、数据与客户业务 owner |
| 覆盖 | SP00–SP15，保留全部 87 个工作项、P1–P16，以及原 REQ / AT / G / I 编号 |
| 实现边界 | 第一方产品与工程运行器使用 Rust；控制台和 TS SDK 使用 TypeScript；不重新定义为知识图谱 |

## 1. 如何使用本 Plan

本文件回答“按什么方法、以什么顺序、由谁、产生什么证据完成实现”。Spec 回答“产品必须具备什么行为”。开发者领取一个工作项时必须同时阅读其 Spec 契约和本 Plan 的执行卡；任务卡没有重复列出的安全要求仍然有效。

这里的工程文件、服务、命令与 CI 是实施交付要求。仓库可能已有部分实现；领取任务时必须核查实际代码、执行台账和验证证据，不能从文件存在推断任务完成。Phase 没有固定周数，测试目标没有被本 Plan 标为已经达成。

### 1.1 指导范式：契约驱动、风险先证、证据闭环

统一执行顺序为：**来源与风险 → 契约和可观察失败 → 最小真实纵切 → 负例/故障恢复 → 集成 → 独立审阅 → 证据签收**。

| 范式 | 每项开发必须做什么 | 禁止替代方式 |
|---|---|---|
| 契约先行 | 固定输入、输出、身份、权限、版本、超时、取消、幂等与故障语义；提供合法/非法 fixture | 先铺服务再由实现反推接口 |
| 风险原型先行 | 对指定 P/DEC 先在真实依赖上证明安全实现；失败登记证据与 ADR | 默认相信示意 SQL、SDK、Docker network 或 Drop 能保证安全 |
| 纵切交付 | 每个能力先做最小可观察路径，包含正确输出、拒绝、取消和恢复 | 只完成 CRUD、只拼 UI、只跑 happy path |
| 权威落在正确层 | 数据授权落在 PG ACL/RLS，目标互斥落在目标事务，撤销落在代理/查询回收/网络 | 把应用 if、控制租约或 prompt 当硬边界 |
| 状态机与资源闭环 | 每步有持久状态、generation、重入规则、资源 owner、终态证明 | 内存标记“完成”、关闭 socket 即宣称后端已停 |
| 测试驱动关键行为 | 对协议、权限、精度、并发与恢复先建立可失败的断言，再实现 | 为低风险排版或机械修改写实现镜像式测试 |
| 证据驱动签收 | 保存实际版本、命令、退出码、结果 hash、环境与独立 review | mock、旧报告、HTTP 200 或空基线代替验收 |
| 客户评测隔离 | 公开合成数据开发；客户 dev 调优，holdout 独立汇总验收 | 客户样本进入公共仓库、公共 CI 或外部评审模型 |

### 1.2 任务、PR 与阶段三个完成层级

任务完成只表示该卡的契约和验收切片已满足；PR 合并只表示相关代码与证据通过审阅；Phase 完成须其全部任务、跨任务集成和退出门禁都通过。原型完成不替代产品验收，组件通过不替代真实用户旅程。

所有记录使用 `planned → ready → in_progress → in_review → verified → done`；失败可转 `blocked`，记录具体缺失与解除条件。客户未到位用 `pending_customer` 标识依赖，不算 done。原型结论分别为 `validated / rejected / blocked`，与产品任务状态分开。

安全不变量优先于便利性。发现方案不能保证授权、零源写、凭证隔离、耐久审计或完整恢复时，停止受影响能力，继续不依赖它的公开工作；不能自行删测试、扩大账户权限或把问题隐藏为成功。

## 2. 工程师与 Agent 的工作协议

### 2.1 领取任务前的检查

1. 阅读工作项、对应 Spec 章节、第 22 章 REQ 追踪和相关来源；确认当前 checkout、未提交修改及文档基线。
2. 写明本任务 owner 与实现者之外的 reviewer。角色可以由同一团队兼任，但作者不能独立批准自己的安全变更。
3. 检查前置 Phase 的签收证据和本卡依赖；组件接口 fixture 就绪可先开发，正式退出仍须真实依赖。
4. 对工具链、SDK、数据库、镜像和许可证等可变外部事实，在首次实施时查官方来源并做真实构建/探针；Spec 候选版本不直接当作可用组合。
5. 确认测试资源、预算、数据可见性和授权；客户未确认不阻止合成实现，不能自行访问或外发客户数据。
6. 基线检查失败时记录已有失败，不能将其归因到新变更，也不能把被破坏的安全基线忽略后继续签收。

2026-10-06补充：[冻结验收计数与复用规范](open-ontology-acceptance-counting.md)定义开发、集成、发行三个独立口径及外部门槛。M1开发24项分母先冻结；审阅限定必要边界，已审阅脚本/报告/构建配方固定来源复用，每次运行保持资源与证据独立。

### 2.2 标准任务卡

2026-10-02执行调整：基础修复使用专用工作线。与其无接口或安全依赖的公共模块可继续开发；受影响能力仍须修复后签收。优先交付真实纵向闭环：PostgreSQL接入→导入与catalog→可审核ontology草案→权限受控查询→CLI对话；随后继续扩展数据库、文件格式、CDC及完整harness。此顺序不删除下文工作项和完整验收要求。

每个可验收子任务明确输入、输出接口、允许写入范围、测试与交接证据。先执行Linux/MSRV局部冒烟，再实施和局部验证；交付后立即提交明确文件，独立复审绑定固定SHA。完整双架构矩阵安排在集成节点；只有新增跨平台风险或失败才提前扩展验证。后续不新建worktree，以单工作区文件分区和共享文件单writer规则执行。

公共开发检查与发行审批分开记录：真实负责人、客户数据、受保护CI、runsc等条件仍是对应验收门槛，不阻塞无关合成fixture及公共功能开发，也不能以开发通过冒充发行批准。进度以已交付功能、已验收工作项和返工率记录；每次交接更新开发台账及ToDo，只有固定提交的验收证据充分时才勾子项，父项必须覆盖完整范围。

87 个工作项使用原 `SPxx-Wyy` 编号。需要拆分时用 `SPxx-Wyy-a/b` 子任务，父任务全部子项签收才完成；不得重新编号或漏掉父项。SP01 以 P1–P16 作为任务编号。

```yaml
task_id: SP04-W02
status: planned
spec: OO-SPEC-001@1.0.1
requirements: [REQ-PL-01, REQ-PL-02]
acceptance_scope: [AT-PL-02@SP04, AT-PL-07@SP04]
depends_on: [SP04-W01, SP04-W05]
owner: pipeline-owner
reviewers: [data-reviewer, security-reviewer]
contract: "COPY/merge/TOAST/墓碑在同目标事务与fence内更新"
allowed_change: "目标Loader、迁移、对应测试与证据；禁止源侧DDL"
inputs: "固定batch/事务顺序/schema/type/hook digest"
outputs: "目标数据、quality、receipt、删除与缺值sidecar"
failure_behavior: "不ACK、不推进完整水位；可重放且旧writer不能提交"
verification: "先合法control，再同key乱序/删除/NULL/断连/强杀"
evidence: "evidence/<run-id>/SP04/SP04-W02/"
stop_if: "TOAST缺失不可辨、source lineage不成立或目标fence失效"
```

该示例是计划，不是真实成功记录；运行时填入实现 commit、依赖版本、精确命令和 artifact hash。每卡通过原工作项关联 Spec 的 REQ / AT，不建立另一套产品编号。

### 当前实施检查点（2026-10-02）

具体固定SHA、证据和writer状态以[开发台账](open-ontology-development-ledger.md)及[纵向集成顺序](../execution/decisions/vertical-integration-order.md)最新记录为准。SDK runtime六项修正获a090批准；P5 IPC有限切片和独立身份补证分别获a091/a100限定验收；源role GUC/main准入修正经a097补证签收，来源revision2已审阅；基础Rust supervisor有限切片获a096批准；目标receipt修正版6cd955a由a108固定复审，类型3ca5357由a110固定复审；结构closure和NFC修正切片已独立批准。CLI query交付3347f5a经a103复审要求FIFO定点返工，根依赖经实际终态和明确交接移交a098 PG无损类型任务。这些开发状态不代表父任务完成。

- [x] SP03-W01-M main源身份安全修正切片：a097固定34fae5d有限approve，原始SQL及新保留Rustoracle完整输出/hash已核验；旧完整stdout仍missing明确保留。仅role GUC/源准入修正，完整W01/catalog/reader/types另验。
- [ ] SP04-W05-a 目标fence迁移：a104交付version3 6cd955a，a108 preflight有限切片已签收；完整fence、Loader及AT-PL-05仍未签收。
- [x] SP04-W05-a1 version3 preflight修正：6cd955a固定复审与原始证据已签收，OO002/旧marker与receipt不变/有限NULL和ACL回归通过；仅test参考路线，F2/F3/F5与生产installer仍待实现。
- [x] SP07-W01-C findings3/4闭包与重复key修正：a086固定eebc0ea独立118cases/0failure批准；完整W01未签。
- [x] SP07-W01-NFC findings1/2/5修正：a094固定3ed558a批准，Linux1.94测试/Clippy0、独立官方Unicode16/17 oracle与产品探针0失败；旧错误hash不静默复用。仅NFC切片，完整W01与供应链另验。
- [x] SP09-W01-a-R SDK六项runtime修正：a090固定bde2e8c批准；Linux1.94测试/Clippy0、独立55必验case通过。完整JSON SDK、供应链/真实Gateway另验。
- [x] SP00-W06-H helper固定依赖图实际audit/deny工具执行证据：a101固定f1a6005、Linux1.94、cargo-audit0.22.2与cargo-deny0.20.2实际退出0，原始输出和hash已核验。仅工具切片，双许可选择/依赖政策准入与完整供应链仍未签。
- [ ] SP00-W06-H-L helper依赖许可证记录闭包：a105交付abd074a五registry记录与Rust验证，a109独立固定复审卡已准备待槽；audited=false，完整供应链/发行未签。
- [x] SP01/P5-IPC-I 独立身份绑定切片：a100固定6831d6e通过production admit_in_worker/--supervise执行十九case，Linux1.94 probe0/fails0；ready nonce/pid/starttime和decision nonce/pid拒绝有原始证据。完整P5/数据库授权/普遍隔离仍未签。

### 2.3 每个工作项的开发步骤

1. **契约 PR：** 固定类型、接口、迁移约束、状态机、合法与负例 fixture、验收范围。没有变更时直接引用已批准契约，不重复造空 PR。
2. **失败探针：** 对本能力建立能检出实际错误的测试。合法 control 证明环境有效；负例必须得到预期错误且无越权副作用，任意 500 不算通过。
3. **最小实现 PR：** 完成一条真实路径与硬边界，同时交付取消、超时、审计及必要恢复；不能先合入可访问的无保护数据能力。
4. **恢复与边界 PR：** 扩展脏值、乱序、并发、断网、磁盘满、进程强杀和重试；只有步骤 3 已满足上线硬约束时，非安全扩展才可拆到后续 PR。
5. **集成与签收：** 对真实依赖执行完整适用 AT，运行接口差分与架构检查；保存证据并由指定 reviewer 核对。
6. **交接：** 提供已实现范围、命令、已知限制、下一项可用输入。禁止一句“完成”而无结果和资源状态。

每个 PR 只有一个主要可审阅行为。任务大于 1–3 个工程日时继续按契约/恢复边界拆分，该粒度是拆分指导，不能作为工期承诺。涉及安全的新能力和必要测试在同一可用交付中启用；安全补丁不能被拆到“以后再做”。

### 2.4 Agent 执行指令模板

> 实现 `<task_id>`。先读取本 Plan 对应执行卡、Spec 对应章节及依赖证据。列出实际前置状态、允许修改范围与验收切片，然后执行契约→最小真实实现→边界/恢复→验证。保持已有用户修改。没有真实证据不得标通过；不访问客户数据或修改源库对象。发现安全方案不成立时记录失败、限制受影响能力，完成可独立工作。最终交接变更、验证命令/退出码、证据路径、限制和下一项依赖。未明确授权时不提交生产部署、消息发送或发布操作。

任务指令不赋予创建其他聊天、发送消息、上线客户生产或批量删除状态的权限。需要多人协作时由项目 owner 分配角色和任务；本 Plan 不要求自动创建子 Agent。

### 2.5 分支、变更与审阅

需要分支时使用 `codex/spxx-wyy-description`，常规任务不要求新 worktree；需要并行隔离时依据仓库状态建立独立 checkout。不得修改 `.specstory/`、个人文件、客户私有目录或其他任务修改。

迁移、协议破坏变更、授权扩张、网络/容器权限变化、依赖/native/unsafe 新增必须在 PR 中显式标记。迁移使用 expand→migrate→contract、checksum 与单执行者锁，定义旧二进制兼容窗口；回滚不能只换镜像而忽略数据和权限。

独立 reviewer 按 Spec 第 19 章分工：组件/Rust owner审契约和资源；安全 owner审 trust boundary、SQL、Secret、网络和释放；数据/领域 owner审类型、位点、metric、码表、实体、日历；运维 owner审部署恢复；评测 owner与独立标注者审 gold/scorer。低风险文档修改走正常 CODEOWNERS，不新增无必要审批。

### 2.6 并行开发与共享文件的修改协议

每次分配并行工作前，先更新[开发台账](open-ontology-development-ledger.md)。台账记录任务、状态、实现者、独立审阅者、前置证据、允许写入目录、共享文件 owner 和执行标识；具体人名尚未确定时明确 pending，不能用角色占位符冒充审阅签名。

| 修改对象 | 分配方式 | 消费者如何继续 |
|---|---|---|
| 根 Cargo.toml / Cargo.lock / 工具链 | 同一时段只有一个写入者，其他任务提交依赖需求 | 使用已冻结依赖或独立原型 workspace；不得各自重生成根 lock |
| oo-core / 公共 schema / OpenAPI | 契约 owner 先合入版本化变更 | 基于明确版本开发；需要新字段时提交契约变更，不复制本地模型 |
| 数据库迁移 / 权限对象 | 按目标 schema 指定 owner，统一编号及执行顺序 | 在隔离测试实例集成；验证升级、重复执行和旧二进制兼容 |
| ci/gates.yaml / 聚合器 / 发布工作流 | 工程 owner 写入，独立安全 reviewer 核查 | 提交新增 gate 和触发路径；不能临时降低 required |
| 组件代码 / fixture / 任务证据 | 每任务拥有互不重叠目录 | 跨目录修改先更新台账；证据使用新 run-id，不覆盖历史失败 |

只有输入契约已冻结且写入区不冲突的任务可以并行。启动消费者之前，记录生产者的版本、hash 和已批准验收切片。发现共享文件正在变化时，将验证结果标为该次快照的结果；集成签收必须在冻结后的统一快照重跑。

合并次序为契约生产者→依赖消费者→集成测试→独立审阅→父任务签收。作者报告测试通过后仍需核对实际产物、命令退出码与修改范围。失败修复创建新证据，保留旧失败和问题处置记录；禁止通过删除负例、替换预期值或复用旧版本报告完成交接。

## 3. 实施顺序、依赖与冻结点

### 3.1 全局执行顺序

| 波次 | 工作 | 可并行条件 | 必须等待的证据 |
|---|---|---|---|
| 0 | SP00 契约与工程基础 | 共享 schema 的修改指定单 owner | 工具链、依赖、fixture、文档和基础 gate 可运行 |
| 1 | SP01 风险原型；SP02 控制基础 | SP02 不受风险阻断的部分可开始 | 具体 P/DEC 通过后才启对应默认能力 |
| 2 | SP03 源；SP06 权限/Gateway 的 fixture 部分 | 先冻结 catalog 和身份契约 | P2/P14/DEC-17；P5/P6/P7/P13/P14 |
| 3 | SP04 Pipeline；SP05 文件；SP06 真实集成 | SP05 先依已冻结 Loader 接口，SP06先用测试数据 | 完整 A/B；源审计；真实写者和PG从库 |
| 4 | SP07 Self-build；SP08 语义发布 | 先SP07基础再SP08；业务领域可独立任务 | Pack、overlay、G门禁、授权与副本发布屏障 |
| 5 | SP09 SDK/CLI；SP10 runtime | SP10镜像/驱动原型可先准备，正式路径等SDK | 统一API；P8/P9/P16；真实runsc隔离 |
| 6 | SP11 释放和产品入口 | 先会话/释放核心，网页与飞书依事件契约开发 | 五秒撤销、六十秒销毁、全旅程、M1 Compose |
| 7 | SP12 公开与私有评测、M1签收 | 私有题库与驻留准备自波次1持续开展 | 客户dev/holdout、UAT、A/B/C、首制品签名 |
| 8 | SP13 MCP；SP14 正式发行 | SP14观测/恢复脚本提前积累，完整退出后验 | 客户k5回归、双架构/离线/恢复/完整A–F |
| 9 | SP15 独立扩展 | 逐扩展独立ADR/feature，不阻既有发行 | 同等安全 conformance 与真实目标环境 |

并行指依赖允许的工作分配，不代表同一共享文件可无人协调修改。接口生产者与消费者都修改 oo-core 时由一个契约 PR 先合并；其他组件跟随已冻结版本。

### 3.2 明确的接口冻结点

| 冻结点 | 生产者 → 消费者 | 必须冻结的内容 | 失效处理 |
|---|---|---|---|
| F0 / SP00 | oo-core → 全部crate | Config/OOS/Batch/Error/Evidence版本、精度、fixture、依赖方向 | 契约diff；破坏变更先更新消费者和版本 |
| F1 / SP02–SP03 | Control/jobs/connector → Pipeline/Builder | plan/apply、lease/generation、Connector/AccessReport、PositionCodec、Batch | 未过P/DEC不得启相应能力；未知权限block |
| F2 / SP04–SP06 | Loader/PG/Gateway → Builder/SDK | 目标receipt/质量/水位、ACL/RLS、只读执行、审计completion | A/B真实回归，不能pool或buffer绕过 |
| F3 / SP07–SP08 | Builder/publisher → Gateway/SDK/runtime | effective OOS、角色Pack、policy/knowledge tuple、发布fence与撤销事件 | 权限收缩终止旧上下文，候选失败保持封闭 |
| F4 / SP09–SP11 | SDK/proxy/runtime → UI/MCP/eval | export completion、事件sequence、session generation、S1–S11 | 断流不完成、取消可证、历史恢复当前权限重验 |
| F5 / SP12–SP14 | eval/release → 运维/客户 | snapshot/suite/scorer/模型/harness/digest、私有签名汇总、迁移兼容 | 版本不匹配拒绝晋级，晋级同一制品digest |

### 3.3 固定默认与风险决策

默认路径为 PostgreSQL Agent DB、Valkey、Rust/Tokio/Axum、Pi、gVisor、Linux Docker Compose standard。TypeScript仅控制台和SDK；Debezium/Pi等外部运行时保持独立协议组件。默认文件托管先LocalFileSystem，再以客户批准的S3实现跑同一contract suite；不默认分发受限许可证服务。

默认是工程实施路线，不把候选版本、上游能力或尚未验证的算法变成事实。P/DEC失败时遵循 Spec 安全替代、返回unsupported/blocked、或经正式ADR改变支持范围；实施者不能自行选择不安全fallback。客户IdP、源对象、驻留、S3后端、硬件与性能预算由客户确认记录，公开实现使用合成fixture/测试IdP继续推进。

## 4. SP00–SP15 执行卡

下列执行卡沿用 Spec 的 87 个工作项。验收编号指 Spec 第 18 章，必须记录范围；各Phase还必须满足Spec第17章退出条件。本章“交付”均表示待创建/实现，读者不能据此认为文件已存在。

### SP00 — 工程基础与共享契约

**输入/前置：** 当前19份docs和Spec；领取时核查实际workspace、台账与已有证据，已有骨架不代表本Phase完成。**负责人/审阅：** Rust owner；Rust与安全reviewer。**顺序：** W01与W06先行 → W02/W03 → W04 → W05集成；W05工具骨架可与W02并行。

#### SP00-W01 — workspace、工具链与依赖方向

- 执行：从官方manifest核验候选工具链/MSRV/features/许可证，锁定可实际构建组合；建立edition2024/resolver3 workspace和各crate最小骨架；先实现禁止依赖边检查，再接入构建。
- 交付/PR：工具链、lock、feature矩阵、dependency-baseline和架构规则；依赖基线与骨架可一PR，新增native另审。Gateway不得链接connector/Loader/DDL，oo-core不得链接服务/DB。
- 验收/停止：AT-CORE-01；干净clone真实locked构建与依赖图。候选不兼容记录阻断与安全替代，不升级/降级后隐去来源差异。

#### SP00-W02 — 类型与版本化契约

- 前置/执行：W01；列出Config/OOS/Batch/HTTP/Error的单位、精度和版本；用Serde/Schemars导出schema与OpenAPI基本类型，建立未知键、重复键、深度、decimal/bigint/null/timezone fixture。
- 交付/PR：oo-core模型、schema、严格解析及差分fixture；先模型再解析，消费者不得各自定义同名不兼容类型。公开接口以Spec第6/12章为边界。
- 验收/停止：AT-CORE-02、AT-CORE-03@SP00仅schema/model；完整SDK链留SP09。类型不确定或精度损失阻止契约冻结。

#### SP00-W03 — 规范值与脏值样本

- 前置/执行：W02类型契约；规定canonical值/PK编码、NULL与缺失区分、时间与浮点特殊值规则；创建PG/MySQL合法及脏值fixture，独立预期hash不得由被测函数生成。
- 交付/PR：规范化模块、版本化hash协议、样本与gold；测试日期/NUL/unsigned/decimal/组合键/collation边界，保留源值→目标值→拒绝的解释。
- 验收/停止：AT-SRC-03@SP00；这里只签规范值fixture，真实驱动与双路径在SP03/SP04验证。无法定义等值时block而非字符串随意比较。

#### SP00-W04 — 合成数据与攻击基础

- 前置/执行：W02/W03；Rust生成OO-Logistics xs/s，固定seed与bad-smell配置；分离公开业务数据、隐藏truth和SQL攻击/角色fixture。
- 交付/PR：生成器、dataset manifest、校验hash、最小授权矩阵；两次生成字节一致，fixture不含客户品牌、域名、样本或Secret。
- 验收/停止：AT-EVAL-01@SP00；此时不标300题/完整D/E已过。生成数据和truth混入产品Pack即阻断。

#### SP00-W05 — xtask、测试环境与Evidence

- 前置/执行：W01骨架及W02/W04契约；实现Rust xtask、cargo alias、ephemeral服务Compose、故障点接口、Evidence schema与聚合gate；未实现suite/缺环境必须非0。
- 交付/PR：基础CI、contracts/architecture/evidence入口、按Phase激活清单；先运行器/fixture，再接真实套件，禁止echo成功。故障feature不得进入发行可触发接口。
- 验收/停止：AT-CORE-04；help smoke、人工篡改schema/期望/追踪后gate真失败，恢复后通过。聚合job漏required、skip变绿即阻断。

#### SP00-W06 — 开源规范与供应链台账

- 执行：先LICENSE/NOTICE/SECURITY/CONTRIBUTING/CODEOWNERS/issue与PR/ADR模板，再依赖审计台账；明确报告漏洞入口与默认禁用许可，CI覆盖Rust/JS/native/镜像依赖。
- 交付/PR：开源贡献文件、依赖发布日期/官方来源/MSRV/license/digest记录和初始安全扫描；不在模板中带客户信息。
- 验收/停止：AT-SEC-05基础切片；新增默认依赖必须合规可核查。禁用许可或已知可利用高危不能靠台账备注放行。

**Phase签收：** 干净clone基础门禁可运行、合法/负例fixture齐备、架构规则可检出违规；全部后续命令明确planned/active状态。SP00不要求先完成A/B完整产品套件。

### SP01 — P1–P16 风险原型与决策冻结

**输入/前置：** SP00薄原型、合成数据、一次性Linux/DB测试节点。**负责人/审阅：** 对应组件owner；安全、数据、Rust、运维，必要客户DBA。**工作方式：** 每P独立spike；先最小反例、再真实规模；同一薄代码可共享但不得让正式服务依赖未验证原型。P表就是本Phase可领取任务。

| 原型 | 明确执行步骤 | 必交证据与停止条件 |
|---|---|---|
| P1 | 制造持续写/删除/fill交错，先小数据查漏复活，再50m；让读从库落后L0并强杀worker | 回放屏障、最终全量0差异、吞吐与资源；未成立禁默认快照发布 |
| P2 | PG/MySQL长事务交错、提交顺序、同位点多事件、epoch切换；审计CDC初始化源对象 | position/txn codec、source lineage、零源写；不可比位点禁修复/一致性宣称 |
| P3 | 真实Debezium offset/history store在提交前后强杀并重启；比对读取与目标位点 | 锁定store和重放算法；不可恢复暂停源并按ADR安全替代 |
| P4 | AOF always峰值写入、磁盘满、重启/切换和日志缺口 | 耐久/吞吐/容量/背压报告；gap必须被识别且隐藏相关表 |
| P5 | 锁parser/PG系列，构建native并跑合法SQL与攻击/星号重写差分 | parser兼容、IPC隔离与许可证；parse失败不能回退放行 |
| P6 | 真从库SET LOCAL/ROLE/claims/RLS、PUBLIC函数撤权、跨用户pool复用 | DEC-03最小授权与执行序列、无claims污染；失败冻结Gateway路径 |
| P7 | 50m不同过滤/JOIN/scan的估计与真实耗时，测EXPLAIN时限 | 硬件/行宽绑定阈值；不拿cost当精确耗时或RLS计数 |
| P8 | runsc真实A/B沙盒，测试peer/IPv6/DNS/host/metadata/IP复用，测预热 | 选定专网或UDS与完整隔离矩阵；普通internal network不算证据 |
| P9 | 锁Pi版本，真实stdio RPC/abort/restore/baseURL/未知事件/断流 | 协议fixture和适配合同；断流不当turn完成，无直连模型fallback |
| P10 | 获准客户样例跑S0–S5；无样例时合成先行，描述盲评与成本测量 | 初始质量/成本、驻留合规；客户未到位保留M0客户pending |
| P11 | Debezium数据/BEGIN/END真实路由，过滤事务计数、rollback、空闲/零所选事件与重放 | 完整applied进度证据与ADR-011；失败不能默认transactional |
| P12 | 50万事件分段、暂停表、同时查询质量/水位，强杀后续跑 | partial标记与完整水位不越过；查询不得虚报一致切面 |
| P13 | 新列/whole-row/CTE/JOIN/star/calendar/entity/掩码raw旁路与角色继承 | 列权限/RLS有效；security_invoker掩码不安全则预掩码/隔离派生或block |
| P14 | 真portal/TLS/迟到cancel/pool、numeric/unsigned/Arrow与双架构native构建 | 驱动契约、精度、工具链/native兼容；不能仅cargo check代替运行 |
| P15 | lease/heartbeat/reclaim/目标fence，逐COMMIT/reject镜像/XACK/XTRIM断点 | 旧owner无提交、数据/receipt/墓碑可恢复；任何漏事件阻断 |
| P16 | native/hook超时杀进程、释放步骤逐一强杀、SIGTERM/panic、Compose/rmcp/offline探针 | 可强制退出与版本协议证据；危险资源未停不能记录成功 |

P项关联Spec原型表的相关AT，不新造另一组验收。每份结论含假设、合法control、反例、环境、原始日志、指标、方案和ADR。50m等规模原型需真实资源；资源未提供可先小规模探索，完整P仍blocked，不能凭线性外推签收。

**Phase签收：** P1–P9/P11–P16有明确安全方案；P10客户基线单独记录。将validated方案写入ADR和兼容矩阵，再让后续产品任务进入ready；迁入产品后重新验收。

### SP02 — 控制面、任务与凭证

**输入/前置：** SP00；使用具体原型结果，不等待无关P。**负责人/审阅：** Control owner；安全与运维reviewer。**顺序：** W01 → W02/W04/W05 → W03；W06依W01/W04。

#### SP02-W01 — 控制库与jobs

开发完成记录（限定子项；完整W01待AT-CTL-01/02）：

- [x] SP02-W01-a 控制schema：70a4e72，a068固定复审批准。
- [x] SP02-W01-c 事件项目绑定：9224d31，a079固定复审批准；真实LOGIN跨项目拒绝、坏历史迁移回滚、合法历史不变。
- [ ] SP02-W01-b Rust领取/续租/generation CAS/schedule/retry及100worker、重启与备份验收。
  - [ ] SP02-W01-b1 受限DB lease原语：a102已派发，新增前向迁移/契约与Rust真实PG fixture，不改共享依赖。SQL会话并发不替代Rust驱动与完整100worker验收。
- [ ] Applicator真实文件哈希/readiness合取、迁移有界超时与40P01重试；事件内容真实性与最小项目scope按后续接口验证。

- 执行：先DDL约束/专用角色/migration锁，再短事务领取、30s lease/10s heartbeat、generation CAS与schedule去重；DB时间为权威，NOTIFY仅唤醒、轮询兜底。
- 交付/PR：oo-jobs与控制迁移、100并发claim fixture、重启/旧owner/retry/cancel测试；业务变更和入队同控制事务。长任务不持领取锁。
- 验收/停止：AT-CTL-01/02；重复计划仅一副作用、旧generation不完成。任务状态不能代替目标DB fencing，目标写者在SP04另验。

#### SP02-W02 — config与Git

- 前置/执行：W01、SP00-W02；实现严格YAML、schema版本/诊断；Git固定argv、根路径和符号链接检查，按commit/generation CAS编辑与读取。
- 交付/PR：config validate、Git适配、诊断API及并发fixture；声明式变更必须可还原到commit，禁止UI绕过Git直接授权。
- 验收/停止：AT-CORE-02、AT-CTL-03；重复键/标签/include/环境插值拒绝，冲突不覆盖。无法绑定输入commit不进入plan。

#### SP02-W03 — validate/plan/apply

- 前置/执行：W01/W02/W04/W05；先计划diff/DAG/预算/风险/期限，再持久apply步骤和资源幂等；用受控fake验证协议后对真PG执行恢复。
- 交付/PR：异步202 operation、idempotency key、generation冲突、checkpoint与Runbook；DDL与授权只在受审apply，不跨库伪造事务。
- 验收/停止：AT-CTL-04；每步强杀可恢复，payload同key不同返回conflict。破坏操作无具体计划、权限或风险说明时block。

#### SP02-W04 — Secret与mTLS

- 前置/执行：W01；AEAD信封/AAD/唯一nonce/密文版本、0600主key或客户KMS；组件最小Secret解析、CA/workload身份分离，轮换覆盖内存和连接池。
- 交付/PR：加密存储/写入/轮换API、访问策略、脱敏、证书续期和故障告警；Secret引用进入配置，明文不进入Git/日志/镜像。
- 验收/停止：AT-SEC-01；nonce/AAD/key错、Gateway读代理namespace、旧凭证有效性测试。轮换失败fail closed，不广播明文会话token。

#### SP02-W05 — OIDC与管理RBAC

- 前置/执行：W01/W04；测试IdP先验证issuer/audience/state/nonce/redirect/PKCE相关流程；bootstrap短期单次、CSRF/Origin、cookie与角色边界。
- 交付/PR：管理身份/会话/审计，测试admin/reviewer/auditor/owner和跨项目负例；管理身份不自动变成数据角色。
- 验收/停止：AT-CTL-05；重放/跨项目/自提权拒绝。生产客户IdP未确认保持pending，测试IdP不能冒充真实接入验收。

#### SP02-W06 — Collector与反馈receiver

- 前置/执行：W01/W04；独立mTLS入口、audit_ingest角色与三元组去重；feedback receiver只允许审核字段/大小/限额，与audit权限隔离。
- 交付/PR：受限接收binary、持久receipt、重放/重复/错误身份fixture；Gateway不获控制库写账号，反馈不自动改OOS/权限。
- 验收/停止：AT-GW-06、AT-CTL-06组件切片；任意管理写或凭证读取被拒。Gateway本地WAL端到端在SP06补验。

**Phase签收：** 控制操作可恢复、输入不可漂移、Secret轮换与管理授权实测；提供F1契约和控制库备份标记。

### SP03 — 只读连接器与类型

**输入/前置：** SP00/SP02，P2/P14/DEC-17。**负责人/审阅：** Data owner；DBA、数据与安全reviewer。**顺序：** W01/W02 → W04/W03 → W05；共同trait先冻结。

#### SP03-W01 — PostgreSQL接入

- 执行：实现Connector/AccessReport的catalog/schema/logic/权限与日志检查；按源版本声明logical/publication/slot/TOAST能力，DBA必要对象只输出清单。
- 交付/PR：PG driver、schema/capabilities和脱敏体检报告；真实只读账号与危险继承角色fixture，源审计覆盖首次启动/重连/no_data。
- 验收/停止：AT-SRC-01；13+支持逐版本有证据，未知权限block，平台零源DDL/写。不得自动创建publication/signaling/slot所需对象。

#### SP03-W02 — MySQL接入

- 执行：沿统一trait实现MySQL8 catalog/charset/grants/ROW/FULL/GTID/log保留/从库检查；验证连接驱动取原值，避免宽松sql_mode暗改脏值。
- 交付/PR：MySQL source contract、只读权限探针与DBA清单；与PG共享fixture结构但分别审计，不推导MariaDB兼容。
- 验收/停止：AT-SRC-01；启动/批量/CDC体检全程零写。账号写能力例外仅已有admin显式计划许可，仍不得实际写源。

#### SP03-W03 — 块规划与有界读取

- 前置/执行：W01/W02/W04；确定稳定键、canonical排序/范围、cursor与读节点；实现Stream背压/连接预算/取消，组合字符串稀疏键不按算术切块。
- 交付/PR：plan_chunks/read_chunk/read_incremental/read_keys、覆盖报告和后端停止探针；多源/小值大值/慢消费者分别验证。
- 验收/停止：AT-SRC-02/04；不重叠遗漏，取消无后端残留，rows/bytes预算可测。cursor不可比或读节点屏障未知拒绝相应一致性模式。

#### SP03-W04 — 类型与PositionCodec

- 前置/执行：SP00-W03与P2/P14；真实驱动双库取值、显式type_policy与reject；位点checked decimal38绑定source identity/epoch/事务序号。
- 交付/PR：类型映射、PK/collation规范、PositionCodec、CDC/批量同值gold；同LSN多事件不得简单max去重。
- 验收/停止：AT-SRC-03/05；unsigned/numeric/timezone/零日期/NULL/NUL保真。溢出/序列倒退/未知epoch暂停，不wrap。

#### SP03-W05 — catalog与注册控制API

- 前置/执行：W01–W04、SP02-W03；source/table/column规范名映射和碰撞处理，注册与体检纳入plan/apply；展示支持模式与真实限制。
- 交付/PR：catalog、lineage/source状态、注册操作API与幂等fixture；保留原名但日志与Agent不泄漏内部拓扑。
- 验收/停止：AT-CTL-04、AT-SRC-04；失败无半注册可用源，未知库CONNECTOR_UNSUPPORTED。未通过体检的源不可启动同步。

**Phase签收：** 真实PG/MySQL源审计零写、类型不静默改值、读取取消与块覆盖证据；提交F1接口给Loader。

### SP04 — CDC、快照与可恢复Loader

**输入/前置：** SP03/SP02，P1/P2/P3/P4/P11/P15。**负责人/审阅：** Pipeline owner；数据、DBA、安全reviewer。**顺序：** W01流协议和W05 fence先冻结 → W02 → W03/W04 → W06；W03/W04均依receipt/墓碑/缺值状态。

#### SP04-W01 — Debezium与事务解码

- 执行：锁镜像digest与offset/history版本，单源有序stream包含所选数据与BEGIN/END；验证过滤后事件计数、rollback、事务顺序、空闲和零所选事件的只读position-complete信号。
- 交付/PR：CDC配置与decoder、事务完整性检查和读取/已见/完整applied三类进度；offset store故障恢复。必要源对象仍归DBA。
- 验收/停止：AT-PL-01/04；P2/P3/P11未证不得声明默认transactional；不能写heartbeat推进源位置，也不能seen冒充applied。

#### SP04-W02 — COPY、merge与sidecar

- 前置/执行：W01/W05与SP03类型；目标持锁同连接事务内COPY、txn_sequence折叠、upsert/delete/fill、quality/receipt/墓碑/TOAST缺值状态；先合法多表事务，再同key update→delete→reinsert。
- 交付/PR：目标迁移、Loader writer、不可用列和删除fence；NULL是已知值，占位字符串碰撞须可靠缺失信号或DBA FULL/绑定补读替代。
- 验收/停止：AT-PL-02/07；旧upsert不复活，fill不盖新值，pending未解不发布。无可靠缺失信号暂停受影响表。

#### SP04-W03 — CDC-first快照与shadow

- 前置/执行：W01/W02/W05、P1；建无用户授权LOGGED shadow，确认读取节点replay≥L0，worker按确定块只读并发送有界fill给唯一目标writer；L_end完整applied屏障后索引/ANALYZE和awaiting_publish。
- 交付/PR：chunks/resnapshot dual-route/generation/replay屏障，rename/OID/RLS/列ACL迁移；撤旧表权限与换表同事务，stage和旧generation不可直查。
- 验收/停止：AT-PL-01/08；从库落后、fill/delete竞态、worker重启、旧表名直查。日志缺口/质量block/未分类/复制屏障缺失均不开放。

#### SP04-W04 — 大事务、暂停与隔离

- 前置/执行：W01/W02/W05；>200k事件耐久spool，分段从首段标partial与cross=false，完整水位保持；schema/TRUNCATE暂停，事件耐久隔离再ACK。
- 交付/PR：有上限spool/quarantine、checksum/完整manifest、段位图、schema变化审核路径；缺事件/残件不当完整，大事务后续不越水位。
- 验收/停止：AT-PL-04/09；50万事件/磁盘满/隔离写半/逐段强杀。无法耐久隔离时停止ACK并背压；TRUNCATE不得默默忽略或直接目标truncate。

#### SP04-W05 — 跨库恢复、租约与ACK/TRIM

- 前置/执行：SP02-jobs与P15；先目标排他锁/事务fence和generation，再目标receipt；reject先幂等控制库耐久，数据/quality/receipt同目标事务，控制镜像/完成/XACK随后可重试。
- 交付/PR：fault ledger、XAUTOCLAIM、trim安全边界、重放去重、不同payload同batch拒绝；COPY/repair/swap不从另一个无锁pool连接写。
- 验收/停止：AT-PL-05；在reject、目标COMMIT、控制镜像、XACK、XTRIM和lease断点逐一强杀。租约检查不等目标fence，旧owner可提交即阻断。

#### SP04-W06 — full/incremental与删除收敛

- 前置/执行：W02/W03/W05；full同RR快照或已证静止窗口；incremental固定W_new、overlap、同cursor全覆盖，完成全部目标应用后控制事务推进W；删行与晚提交依能力披露并对账。
- 交付/PR：batch调度、run generation、snapshot/spool生命周期与质量状态。无CDC full快照失效且无完整耐久spool则废弃整shadow重读；增量失效则新run/快照/上界重读，不推进旧W。
- 验收/停止：AT-PL-03/06；同timestamp多次变更、快照连接失效后源变化、旧/新块混合探针。部分应用保留best_effort/cross=false；>300s不静默延长一致快照。

**Phase签收：** A(s100k)全过并永久required；5m nightly与50m原型证据；停源写追平后全量0差异、各故障可恢复、源预算测量。首次能力PR即携真实正确性测试，不等最后补齐。

### SP05 — 文件、dump与对象生命周期

**输入/前置：** SP02、SP03类型、SP04 Loader契约、P16。**负责人/审阅：** File owner；安全、数据与运维reviewer。**顺序：** W01 → W02/W03/W04 → W05 → W06；下载ACL/临时清理从W01即具备，W06完成全生命周期。

#### SP05-W01 — 上传与对象ACL

- 执行：限定大小/配额/格式/multipart、checksum和idempotency；local与批准S3用统一object_store接口；对象key由平台生成、project/principal鉴权不靠猜不到路径。
- 交付/PR：上传session/parts/complete/abort、对象manifest、ACL与残件清理；先local真实测试，再选定S3真实contract，不默认引入MinIO。
- 验收/停止：AT-FILE-01/05；断网/重复part/并发complete/跨用户/篡改hash。失败不公开残件，资源不足停止接收而非无限缓存。

#### SP05-W02 — CSV/TSV/JSONL/Parquet

- 前置/执行：W01和类型契约；格式/编码/BOM/delimiter严格解析与预算，DuckDB/native独立可杀进程；Parquet footer/schema/深度/压缩比/单值上限校验。
- 交付/PR：格式parser worker、受限进程协议、profile/错误fixture；逐格式子PR共享Arrow IPC，恶意数据不能影响主服务。
- 验收/停止：AT-FILE-02；编码坏值、超长行/嵌套、压缩炸弹/截断流。猜测格式不确定返回预览确认，不能静默吞行。

#### SP05-W03 — Excel

- 前置/执行：W01/SP00类型；calamine隔离读取xls/xlsx，展示sheet/header/重复列预览；1900/1904日期、公式缓存、合并单元格和中文fixture分别核验。
- 交付/PR：Excel worker与parse_profile，保留源行/单元格lineage；不执行宏、外链或公式，缓存缺失有明确行为。
- 验收/停止：AT-FILE-03；zip炸弹、公式/超界日期/恶意路径与大sheet。异常可杀退出、临时对象清零，不把空缓存当计算值。

#### SP05-W04 — SQLite与SQL dump

- 前置/执行：W01、P16；SQLite只读模式禁扩展/危险pragma/附加路径；dump进入无源凭证、无网络、受限恢复账号的一次性容器，输出经统一类型路径。
- 交付/PR：导入版本矩阵、沙盒payload白名单、恢复工具digest、失败回收；SQLite与不同dump恢复器分PR，不做万能宿主SQL执行。
- 验收/停止：AT-FILE-04；恶意DDL/函数/COPY外文件/网络/restore超时。未列支持版本明确拒绝，恢复容器不接触客户运行库。

#### SP05-W05 — 文件导入与数据策略

- 前置/执行：W02–W04和Loader；profile审核后按replace/append/upsert应用，主键/重复/类型策略固定；取消与重试绑定输入object hash，不改用新文件。
- 交付/PR：文件import状态机、lineage、generation/receipt、预览到发布的进度；replace复用LOGGED shadow及授权发布屏障。
- 验收/停止：AT-FILE-06；三策略正/负例与断点恢复，失败无部分公开表。无稳定键禁upsert，配置/hash漂移返回conflict。

#### SP05-W06 — 下载、删除与保留

- 前置/执行：W01/W05；下载经鉴权代理和当前权限tuple，TTL/版本/缓存/multipart/temp统一删除；输出CSV/Excel公式注入转义不改变内部Parquet数值。
- 交付/PR：artifact/下载元数据、reaper与删除证据；旧URL撤权≤5s拒绝，不用直连长期对象URL绕过即时授权。
- 验收/停止：AT-FILE-05、AT-SEC-03；跨用户、过期、降权、篡改URL/公式/目录穿越。无法证明当前权限匹配时拒绝旧文件而非仅验同用户名。

**Phase签收：** 首版所有格式真实fixture通过，local/S3能力分别标记；无残件、危险解析宿主副作用、部分公开表或文件授权旁路。

### SP06 — PostgreSQL授权与Gateway

**输入/前置：** SP02、SP03 catalog、P5/P6/P7/P13/P14；正式质量集成依SP04。**负责人/审阅：** Gateway/DB owner；安全reviewer必须独立签字。**顺序：** W01和W06准入审计先有最小实现 → W02/W03 → W04 → W05 → 全B。

#### SP06-W01 — 数据库权限编译

- 执行：NOLOGIN用户角色、列GRANT、RLS/helper与PUBLIC/default/function权限撤销；绑定只取首个角色，claims参数化平台先设再SET LOCAL ROLE；新列/未知PII默认不可见。
- 交付/PR：policy compiler与实际ACL/继承/RLS/view drift巡检、受限calendar/派生资产规则；测试绕过应用检查时数据库仍拒绝写/隐藏列行/claims修改。
- 验收/停止：AT-DB-01/02/03；pool串角色、whole-row/函数/operator/cast/SECURITY DEFINER探针。raw掩码旁路未证安全则block配置。

#### SP06-W02 — SQL安全静态规则

- 前置/执行：W01、P5；parser/PG匹配、预算和可杀隔离；R1–R9、scope-aware star、函数OID白名单、CTE/JOIN/子查询/危险utility/写副作用验证。
- 交付/PR：SQL guard、executed/original SQL区分、合法误拒集；不能用字符串正则做唯一权限检查，不回显隐藏错误/EXPLAIN。
- 验收/停止：AT-GW-01/02；合法control与每规则负例、攻击差分。未知AST/函数/解析错误拒绝，不能fallback直接数据库执行。

#### SP06-W03 — token与准入限流

- 前置/执行：SP02身份/Secret、W01/W06；token仅hash存储，audience/TTL/role/currentpolicy检查，撤销传播；公平队列/permit/限额有界且generation防双释放。
- 交付/PR：mTLS/token签发校验、rate/concurrency、保守降级与负例；签发入口与代理workload身份分开，role/token不能自报。
- 验收/停止：AT-GW-03/07；跨受众/过期/降权/多实例预算与依赖失效。state不可证时封闭或收紧，不缓存旧许可继续开放。

#### SP06-W04 — 查询执行与可证取消

- 前置/执行：W01–W03/W06、P14；RR READ ONLY、EXPLAIN预算、prepare/portal分页与总wall-clock deadline；取消带backend_start/connection generation，ROLLBACK/reset后才pool复用。
- 交付/PR：executor、后台登记/可信最小权限query reaper契约、慢客户端背压；30s query/120s export/3s search不被逐页Execute重置。
- 验收/停止：AT-GW-04；断连/迟到Cancel/pool跨用户/超时/热备冲突。关闭socket不当后端已停；未知pool状态销毁并报警。

#### SP06-W05 — 精确结果、质量与freshness

- 前置/执行：W04、SP04元数据；typed结果decimal/bigint字符串、时区语义、max+1截断；同查询快照读取质量/水位并按lineage/当前RLS过滤warnings和统计。
- 交付/PR：result/error/warning模型、安全血缘、query hash、star-expanded与metadata；未知血缘不发布，跨源不宣称全局一致。
- 验收/停止：AT-GW-05；partial/拒绝/未知码/过期/实体质量与RLS隐计数探针。不能用截断文本hash伪证全量结果。

#### SP06-W06 — 准入审计与completion

- 前置/执行：SP02-W06；对每调用先耐久admission再任何结果字节，完成/拒绝/cancel独立事件；本地独立卷fsync WAL、三元组重放与Collector确认后GC。
- 交付/PR：audit spool/admission/end/query completion及read-own接口；SQL敏感literal受限加密/脱敏，普通日志只有fingerprint。
- 验收/停止：AT-GW-06/08；磁盘满/fsync失败/Collector停机/WAL重启不漏；失败AUDIT_UNAVAILABLE且封闭数据，不无界重试。

**Phase签收：** B全部100%、合法误拒目标<2%、真实主从和minimal只读兜底、A数据质量集成。F2冻结；首次访问能力PR必须具备真实授权/审计负例，不能暂时关闭门禁。

### SP07 — Self-build、审核与角色Pack

**输入/前置：** SP02/SP03、SP04或SP05真实数据、SP06只读审核访问。**负责人/审阅：** Builder owner；领域/数据与安全reviewer。**顺序：** W01 → W02 → W03/W04 → W05 → W06；规则草稿路径先于LLM路径。

#### SP07-W01 — OOS与overlay

- 执行：严格OOS schema、generated/curated/effective三层，列表按稳定ID合并；rejected/override/evidence hash绑定事实版本，输入变化标stale等待审核。
- 交付/PR：overlay compiler、semantic diff、gold fixture；Builder只能generated，人工curated不可覆盖，列表重排不改变语义。
- 验收/停止：AT-OOS-01；重复Builder/事实变化/拒绝项/删除引用。schema未知或没有稳定身份不能靠数组下标合并。

#### SP07-W02 — S0/S1结构与画像

- 前置/执行：W01、授权catalog和数据；读取schema/logic/sample/profile，记录预算、时间窗口/偏样本；cache绑定source/schema/config/type/evidence版本。
- 交付/PR：结构画像、证据链与输入hash；结构/统计与领域结论分开，无数据或受限样本展示不确定性。
- 验收/停止：AT-OOS-02；无注释/FK/偏时间窗口、零样本和权限限制。统计不能泄露未授权区域规模。

#### SP07-W03 — S2–S4规则与专家卡

- 前置/执行：W02；类型/PII/关系含基数/码表签名/别名与时间锚点规则，输出候选置信度和证据；规则自动接受只限Spec许可项。
- 交付/PR：规则模块、D真值fixture、专家卡与分类待办；关系冲突/弱键不得直接自动合并实体或认证metric。
- 验收/停止：AT-OOS-02/03；PII漏判fixture为0，未知保留unknown。高置信业务猜测不能代替权限/业务owner审核。

#### SP07-W04 — S5结构化LLM

- 前置/执行：W01/W02、Builder专用Secret与驻留许可；只传获准证据，结构化JSON验证/预算/重试/缓存，注释/样本/feedback均当不可信数据。
- 交付/PR：模型adapter、prompt/输出版本、tokens/成本/耗时报告和故障注入；无LLM仍能生成结构稿，LLM只生成审核候选。
- 验收/停止：AT-OOS-06；prompt注入/格式错/断流/服务不可用无curated或发布副作用。客户驻留未批准禁外发样本。

#### SP07-W05 — 分类审核与owner证据

- 前置/执行：W01/W03/W04、管理RBAC；审核S9/S10、批量分类预览、PII/semantic_type强制完整，认证绑定对应owner与证据hash。
- 交付/PR：review API与最小console卡、审核决定/操作者/版本；先后端权限与审计再界面，不因“全部接受”绕过高风险项。
- 验收/停止：AT-OOS-03；reviewer试授权/认证、审核后替换证据、事实stale。未分类列继续隐藏。

#### SP07-W06 — 角色Knowledge Pack

- 前置/执行：W05、SP06policy；编译只含当前角色可见OOS/示例/统计/资产，目录预算/manifest/hash；API和沙盒挂载使用同不可变产物。
- 交付/PR：Pack compiler、path规范、principal/policy/knowledge缓存键、离线角色fixture；输出受限metadata并拒绝路径穿越。
- 验收/停止：AT-OOS-05；跨project/role/RLS样本与统计、缓存串权、预算。授权漂移或未证血缘拒绝Pack发布。

**Phase签收：** 真实结构/规则/LLM候选可审核、D组件指标与Pack安全报告；完整真实会话C0–C3消融到SP12，不制造SP07依赖SP11的循环。

### SP08 — 业务语义、质量修复与发布

**输入/前置：** SP04/SP06/SP07。**负责人/审阅：** Semantic/publish owner；metric/enum/entity/calendar业务owner、数据与安全reviewer。**顺序：** W01/W02/W03和W05 → W04 → W06；W04状态机可先用签名fixture，真实客户G10在SP12。

#### SP08-W01 — enums与业务日历

- 执行：枚举有效期/重叠/未知巡检与安全统计；calendar财年/季度/周/时区/DST/结账记录，受限owner视图仅calendar必要列且无业务表权限。
- 交付/PR：enum compiler/sweeper/calendar helper与独立预期日期fixture；不同领域规则分PR，历史更义和结账审计不可丢。
- 验收/停止：AT-OOS-07、AT-DB-04；未知值warning、历史as_of、边界日期/越权配置。缺业务锚点不能用自然月偷偷替代。

#### SP08-W02 — metric与语义生命周期

- 前置/执行：W01、effective OOS；参数模板采用安全绑定与标识符白名单，认证对独立control_total；literal/粒度/过滤/anchor变化触发semantic diff和owner审阅。
- 交付/PR：metric compiler、认证报告、deprecated/replaced_by和引用检查；命名变更走新增+弃用，不能悄改历史指标含义。
- 验收/停止：AT-OOS-08/09；注入/金额scale/否定码/独立总数漂移。G5a/G5b失败不能force认证。

#### SP08-W03 — 实体与否定决定

- 前置/执行：SP02 key registry、RLS和领域owner；强键自动合并、候选人工决策、canonical否定pair、合并/拆分/alias/members有效期，key永久不复用。
- 交付/PR：决策状态机、历史实体与金额守恒检查、各角色安全display/resolve；弱名称/相似度只候选，用户确认不直接写alias。
- 验收/停止：AT-OOS-10；坏强键/负面关系/拆分/碰撞/隐藏成员。守恒或权限失败不发布实体层。

#### SP08-W04 — G1–G13与原子可见发布

- 前置/执行：W01–W03/W05、SP07 Pack；不可变candidate/gate report，封闭受影响角色全部新旧访问并停止流，再目标授权/资产事务、replay+G7-post、CAS active、开放兼容访问。
- 交付/PR：publish checkpoints/role fence/rollback、每G失败fixture、candidate输入hash；G7-pre检查当前drift，G7-post检查候选真实ACL与全部服务副本。
- 验收/停止：AT-OOS-04/11；GRANT已回放但CAS失败、并发publish、每步骤强杀。失败保持封闭，回滚不恢复已撤销旧权限；G10fixture不算客户结果。

#### SP08-W05 — 对账与受控修复

- 前置/执行：SP04 receipt/fence/codec；对账执行同digest的type/hook/filter、canonical PK桶与multiset；修复P1→读节点replay≥P1→一致快照读键→P2→完整applied≥P2→目标持fence重检后写。
- 交付/PR：hook受限容器、hash/count/diff报告、repair/resnapshot状态、quality清除条件；sampled覆盖明确、count扫描计入源预算。
- 验收/停止：AT-PL-06/10、AT-GW-05；修复从库落后/并发CDC-delete/lineage切换，native超时强杀。位点/快照不可靠改受审resnapshot，不覆盖新值。

#### SP08-W06 — 审核发布控制台

- 前置/执行：W01–W05及Control API；逐页实现草稿、分类、指标、码表、实体、权限预览、gate/进度/失败恢复，按钮与后端同RBAC。
- 交付/PR：TypeScript页面、typed API、操作审计与e2e；分页面纵切，UI不直连DB/Secret、不自行跑DDL。
- 验收/停止：AT-UI-01；未授权隐藏只是体验，后端必须拒绝；失败清楚呈现block/warning。force仅限Spec可豁免非安全项。

**Phase签收：** 每gate正负fixture、真实权限副本屏障、实体守恒与指标独立总数；F3冻结。真实会话终止在SP11再验、客户holdout/G10在SP12再验，未验部分保持pending。

### SP09 — SDK、CLI、导出与本地分析

**输入/前置：** SP06、SP08公开schema。**负责人/审阅：** API/SDK owner；安全、数据与客户端reviewer。**顺序：** W01 → W02/W04 → W03 → W05；export模型与Gateway completion先合同后两端实现。

#### SP09-W01 — Rust SDK

- 执行：只经Gateway HTTP，typed request/result/error，外部认证与from_sandbox_env分离；只读代理URL的沙盒client无OO_TOKEN/home凭证；参数只可收紧role硬限。
- 交付/PR：oo-sdk、公开方法与真实HTTP fixture；重试最多3次且只对有限retryable错误，SQL超时/已开始export不盲重跑。
- 验收/停止：AT-API-01；与HTTP精度/权限/截断一致。SDK不持数据库驱动或独立业务结果缓存。

#### SP09-W02 — CLI

- 前置/执行：W01；数据/管理命令独立认证，config validate与sql validate无权限猜测；JSON stdout纯净、stderr进度、Markdown安全footer和退出码。
- 交付/PR：oo CLI/help/命令解析/输出fixture；保留Spec兼容入口但歧义报usage，不根据目录选管理权限。
- 验收/停止：AT-API-02；0/2/3/4/1退出行为、拒绝hint/warnings、恶意数据伪造footer。沙盒CLI无法读取/签发管理token。

#### SP09-W03 — export完成协议

- 前置/执行：W01/W02、Gateway completion；Arrow IPC EOS校验、临时Parquet、查询completed元数据、规范hash/row核验后原子rename与meta；IPC与Parquet字节数分别统计。
- 交付/PR：SDK/CLI export、完成查询接口fixture、文件meta；任意HTTP200/EOF不能单独判成功。
- 验收/停止：AT-API-03；断流/缺EOS/hash错/未耐久completion/cancel清理或隔离残件。truncated成功必须显式说明，禁止两次快照拼接导出。

#### SP09-W04 — TypeScript SDK

- 前置/执行：F2/F3和W01fixture；OpenAPI生成基础client+手写易用层，JSON精确数值保留字符串；Node/浏览器能力显式区分。
- 交付/PR：生成流程与contracts diff，Node同fixture、浏览器不导出服务器/本地路径、不嵌Secret。
- 验收/停止：AT-API-01与AT-CORE-03@SP09完整链；HTTP/Rust/TS/CLI的字段、错误、warnings一致，生成物漂移阻断。

#### SP09-W05 — oo-analysis与文件产出

- 前置/执行：W03；只读授权Parquet本地DuckDB/Arrow分析，中文图表/安全CSV/Excel与来源manifest；按资源预算隔离native并可杀退出。
- 交付/PR：Rust analysis CLI、CJK字体与工具镜像清单、query_id/输入hash/warnings关联；不让分析工具持数据库凭证或任意外联。
- 验收/停止：AT-API-04；精度/中文图表/大文件取消/公式安全与lineage。产出无可核验输入则标不可引用，不能冒充业务证据。

**Phase签收：** 三类客户端与HTTP同契约、完整导出失败路径、安全本地分析；F4工具部分冻结，测试不是只有序列化单元mock。

### SP10 — gVisor、出口代理与Pi

**输入/前置：** SP02/SP06/SP09、P8/P9/P16，一次性Linux runsc节点。**负责人/审阅：** Runtime owner；安全与运维reviewer。**顺序：** W01 → W02/W03 → W04 → W05；网络和凭证隔离先于真实模型/数据会话。

#### SP10-W01 — SandboxDriver与编排边界

- 执行：实现create/put_files/exec_stdio/get_files/destroy/list，容器payload平台生成并校验digest/runsc/non-root/capdrop/read-only/resources/mount；tar拒绝穿越/链接/device。
- 交付/PR：受限编排服务、driver和真实资源退出探针；Docker socket不挂沙盒，文件流/列表分页有预算。
- 验收/停止：AT-RT-01、AT-SEC-04；privileged/host挂载/network/任意镜像、跨session attach/rm拒绝。runsc缺失生产fail closed，无普通runc静默替代。

#### SP10-W02 — 每会话网络与身份映射

- 前置/执行：W01、P8；按验证结论选专属internal network+proxy attachment或UDS；source身份/IP映射绑定generation，新旧映射撤销确认后再复用。
- 交付/PR：网络策略与mapping状态、A/B沙盒连通矩阵；测试IPv4/IPv6/DNS/metadata/host/peer/DB/Docker，不能只测公网失败。
- 验收/停止：AT-RT-02/03；每沙盒仅专属proxy/socket。共网peer可达或IP复用串权立即封闭会话。

#### SP10-W03 — 固定出口proxy

- 前置/执行：W02、Gateway token；显式数据/v1方法路径与批准LLM origin白名单，禁CONNECT/任意Host/redirect/DNS rebind；entry与egress workload权限分开，代理注入Secret并脱敏全部响应通道。
- 交付/PR：路由/headers/TLS/mTLS、stream背压/deadline、原子usage预算预留结算；会话token仅代理内存或专属加密namespace，模型key只Secretref。
- 验收/停止：AT-RT-03、AT-GW-03；/internal、/control、编码绕过、echo凭证、多实例预算。upstream不可证取消或Secret回显即安全失败。

#### SP10-W04 — Pi真实适配

- 前置/执行：W01–W03、P9；锁版本、RPC stdio launch/send/events/abort/restore，事件sequence/大小/未知类型检查；baseURL固定proxy，无真实APIkey入harness。
- 交付/PR：Pi adapter、统一事件fixture、transcript/restore协议；其他harness仅独立feature，不因可运行Node而假设RPC兼容。
- 验收/停止：AT-RT-06；真Pi断流/工具错误/abort/restore/模型fallback。断流标失败或incomplete，不能turn_finished。

#### SP10-W05 — 工具镜像、Pack与预热

- 前置/执行：W04、SP09；固定Pi/Node/CLI/native/CJK/rg/jq/fd版本与digest；只读Pack，空workspace预热槽CAS一次绑定，使用过的容器永不回池。
- 交付/PR：镜像manifest/扫描、Pack挂载和预热pool；运行时不install/download，资源CPU2/mem4GiB等沿Spec初始预算。
- 验收/停止：AT-RT-01/07；canary Secret扫描0、多用户争槽、超预算与残留。预热出现用户数据或凭证立即销毁。

**Phase签收：** 真实Linux/gVisor C安全全过，多沙盒并存隔离、代理不给Agent任何可复用凭证；runtime敏感PR起C与释放套件required。

### SP11 — 会话释放、网页与飞书产品入口

**输入/前置：** SP08/SP10，F3/F4与可信query reaper。**负责人/审阅：** Entry/runtime owner；产品、安全与运维reviewer。**顺序：** W01 → W02 → W03；W04/W05依事件与释放安全骨架，W06完成集成和M1安装。

#### SP11-W01 — session/turn持久状态

- 执行：conversation/session/turn、owner lease/generation CAS、当前principal/单role/claims/policy/knowledge tuple；入口验签/OIDC后映射身份，用户自报claims无效。
- 交付/PR：状态机、并发/预算/event_id去重、换提问人新sandbox；idle30min/absolute8h/turn10min和工具60次硬限可观测。
- 验收/停止：AT-RT-05/07；跨人/跨项目/抢槽/重放/预算超限。身份不清或授权改变终止/拒绝，不能共享前人workspace。

#### SP11-W02 — S1–S11释放与reaper

- 前置/执行：W01、Gateway backend登记；持久t0与steps，立即并发关闭准入、取消查询、revoke token/mapping/permits≤5s，不等待harness10s；S6/S7后收集，再≤60s销毁并验证资源不存在。
- 交付/PR：release orchestrator/checkpoints/deadlines、专用query reaper、孤儿对账；S10独立5s路径，S8未确认不能closed；30s检测owner失联并报告总孤儿时长。
- 验收/停止：AT-RT-04/08；每步骤强杀/Entry失联/Valkey故障/PG不可达/无限输出。S2 backend未证停止记failed/SLO miss并封闭，drop socket不伪报成功。

#### SP11-W03 — 记录、outputs与当前权限恢复

- 前置/执行：W02、SP05对象ACL；S3–S5限预算、类型/大小/path校验、checksum和凭证扫描、加密存储；optional workspace默认关闭TTL24h。
- 交付/PR：产出manifest、transcript version tuple、restore/download重授权；tuple不完全匹配初版拒绝旧数据，安全裁剪需独立证明。
- 验收/停止：AT-FILE-05、AT-RT-05；降权后旧URL≤5s拒绝、换人/旧Pack不能恢复。收集超时记录artifact_loss，优先隔离销毁不延长存活。

#### SP11-W04 — 网页对话

- 前置/执行：W01/W02事件与SDK；chat/重连sequence/warnings/SQL query_id引用/outputs，安全Markdown禁止script/iframe/外图/自动link preview。
- 交付/PR：网页纵切与e2e、query真实性校验、仅本人conversation/artifact；缺口/歧义澄清与失败展示保留语义。
- 验收/停止：AT-UI-02、AT-SEC-03；恶意HTML/伪query引用/事件断流/跨人访问。UI不能因stream EOF显示回答已完成。

#### SP11-W05 — 飞书渠道

- 前置/执行：W01/W02、客户企业身份映射；真实签名/时间窗/重放/消息去重；群默认仅状态、完整结果私聊，私聊失败不fallback群。
- 交付/PR：channel adapter、合成回调fixture与获准企业UAT；full_in_group需已审同权限群且变成员/策略撤销，敏感角色一直私聊。
- 验收/停止：AT-UI-03；伪签名/群换人/私聊失败/权限变更不泄漏。开发测试不用工具给真实成员擅发消息。

#### SP11-W06 — 管理运维页与M1安全启动

- 前置/执行：W01–W05和SP08console；源/任务/会话/审计/停止页面统一RBAC；实现单目标架构M1 standard Compose、预编译CLI up/preflight/health/一次性bootstrap。
- 交付/PR：完整Sources→Publish→Chat→Release演示、repeat up幂等、主从/buffer-state/WAL卷网络；紧急停止不依赖模型或页面继续运行。
- 验收/停止：AT-UI-01、AT-OPS-05、AT-OPS-01@M1；runsc/磁盘/端口/迁移/审计/权限ready预检。双架构/离线完整矩阵SP14，不能此时宣称已全过。

**Phase签收：** 真实端到端关闭旅程、无状态/资源泄漏、权限发布会话撤销与M1可安装；F4完整冻结。客户IdP/飞书真实接入在交付前补验。

### SP12 — 公开Bench与客户MVP

**输入/前置：** SP04–SP11、SP00持续合成数据；客户接入资料自早期持续收集。**负责人/审阅：** Eval/customer owner；独立标注者、客户DBA/业务/安全。**顺序：** W01/W04 → W02/W05 → W03 → W06；private holdout仅评测管理员管理。

#### SP12-W01 — 数据、300题与gold

- 执行：完成Rust xs/s/m/l生成与全部bad-smell；公开300题含角色/意图/锚点/warnings/SQL/control totals，每题gold由业务分析师与工程师成对确认，至少20%双人独立标注，有分歧仲裁。
- 交付/PR：冻结dataset/suite manifest、truth隔离、题版本和签收；不能把生成器自输出当scorer独立正确性证据。
- 验收/停止：AT-EVAL-01/02；确定性、gold不进入Pack/trace prompt，bad-smell与D全部指标有n/结果。缺gold或歧义未定不记满分。

#### SP12-W02 — 冻结副本与独立会话运行

- 前置/执行：W01、runtime；snapshot+固定as_of/eval Gateway/token隔离生产，每题每次独立会话，同模型/harness/role/预算/版本，完整关闭后再运行下一次。
- 交付/PR：Rust eval runner、snapshot/seed/version tuple、试次与资源证据；C0–C3/C4消融同权限同数据，未分类数据不因裸schema放权。
- 验收/停止：AT-EVAL-03；指向生产/不同snapshot/时区边界拒绝。水位/SQL hash不能代替历史冻结副本。

#### SP12-W03 — 评分、统计与报告

- 前置/执行：W01/W02；decimal/null/multiset/ties/not_found比较、warnings/引用/诚实判断，校准judge；pass^k按每题全部k次实际通过计算，paired/bootstrap按题保留完整试次向量抽样。
- 交付/PR：scorer版本、独立已知正负fixture、D全部指标和成本/人工审阅、HTML/Markdown报告；metadata日期/query_id数字不误作业务编造。
- 验收/停止：AT-EVAL-02/04；已知label100%匹配、手算k3/bootstrap、不可比版本拒绝。gold坏项needs_reannotation保留门禁失败，不能删题抬分。

#### SP12-W04 — 客户接入与安全部署

- 执行：私有onboarding记录源版本/DBA对象/读取预算/驻留/IdP/RLS/角色/业务owner/硬件/备份留存；先低风险表集完成源审计和全量对账，再发布有限角色。
- 交付/PR：私有接入清单与M1标准安装/升级前检查、职责签收和数据驻留证据；公共仓库仅模板及合成示例。
- 验收/停止：AT-EVAL-05、AT-OPS-01@M1；双架构/离线在SP14。驻留/身份/权限未批准不得打开真实Agent会话。

#### SP12-W05 — dev、holdout与live闭环

- 前置/执行：W04、客户业务标注；dev≥150/holdout≥150，holdout细题/gold/trace仅独立管理员，产品团队只汇总；季度轮换1/3、E01–E17人工确认归因。
- 交付/PR：私有ACL/无公网runner、题版本、dev修复→holdout汇总→live补题流程；只客户批准汇总导出且低样本维度抑制。
- 验收/停止：AT-EVAL-05/06；工程账号读取holdout或导出明细拒绝。不得逐题holdout调优，不用客户数据给公共CI/模型评审。

#### SP12-W06 — MVP全旅程与签收

- 前置/执行：W01–W05、SP11；真实UAT覆盖注册/上传→同步→self-build→审核→发布→问答→文件→关闭，A/B/C安全和客户E统一版本。
- 交付/PR：用户签收、问题/角色/期望/实际、holdout pass^3/诚实/可追溯与发布报告；首M1制品已有签名/SBOM/NOTICE，不等SP14。
- 验收/停止：AT-UI-04、A/B/C/E客户结果；安全100%、源零写、holdout初始≥70% pass^3须客户确认、诚实≥95%、观察编造0、可追溯100%。客户缺数据/题库不标M1完成。

**Phase签收：** 公开Bench可重现、私有holdout隔离、客户门禁与UAT完成；用真实结果补G10，F5评测部分冻结。

### SP13 — MCP、外部harness与持续建议

**输入/前置：** SP09/SP11/SP12、rmcp兼容证据。**负责人/审阅：** MCP/auth owner；安全、API与评测reviewer。**顺序：** W01协议与W02授权同行 → W03；W04独立语义建议；W05按扩展选择启用。

#### SP13-W01 — MCP transport与包装

- 执行：官方rmcp stdio/Streamable HTTP，tools/resources/prompts包装同Gateway；版本协商、Origin/session路由与实际HTTP差分，stdio日志只stderr。
- 交付/PR：MCP adapter和目标客户端协议fixture；目标/上一协议版本必须真实兼容，不从SDK版本推导。远程不提供服务器文件系统export工具。
- 验收/停止：AT-MCP-01；结构/文本/精度/warnings/资源权限等同HTTP，未支持版本明确失败。上线transport须同时满足W02授权。

#### SP13-W02 — OAuth与token exchange

- 前置/执行：W01/身份平台；资源metadata/resource indicator、S256 PKCE、state/nonce/redirect exact allowlist、单次code、refresh/revoke；外部MCP audience换内部Gateway audience且scope/role只收紧。
- 交付/PR：OAuth flow与exchange、每HTTP请求认证/Origin校验；MCP session ID只路由，原外部token不透传Gateway。
- 验收/停止：AT-MCP-02/03；跨受众/issuer/scope/redirect重放/原token透传均拒绝。rmcp不自动替OO保证OAuth安全。

#### SP13-W03 — 外部token与角色能力

- 前置/执行：W02和管理RBAC；token UI只签发时展示明文、TTL/revoke/audience/role约束，role可禁外部；管理员看数据驻留与外部工具风险再授予。
- 交付/PR：token管理/审计、资源cache绑定principal/policy/knowledge，撤权立即失效；外部harness不承诺其他工具无法外传。
- 验收/停止：AT-GW-03、AT-MCP-03；跨role resource cache/旧token/撤销故障探针。不因一键连接跳过明确身份授权。

#### SP13-W04 — S6–S8增量建议与反馈

- 前置/执行：SP07/08语义与SP02 receiver；将gap/拒绝/失败聚合为审核候选，控内容大小/去重/驻留，保持generated/curated与授权边界。
- 交付/PR：建议job、证据来源与review页面；可信指标与不可信feedback分别标明，接受后走正常G门禁。
- 验收/停止：AT-OOS-06、AT-CTL-06；反馈prompt注入/跨项目/重复/伪证不扩大权限、不自动发布。

#### SP13-W05 — 可选harness适配

- 执行：默认Pi保持基线；Slack/Claude/Codex等逐adapter独立feature、版本协议、launch/events/abort/restore、身份与输出边界，未选择不进默认包。
- 交付/PR：每adapter单独支持矩阵、C/E公开和客户k5比较；不可用版本blocked，不用示例客户端当真实协议测试。
- 验收/停止：AT-RT-06、C/E k5；升级不降安全/质量，异常事件/断流不完成。没有可用授权/客户端的扩展保留disabled，不阻默认交付。

**Phase签收：** 已支持远程客户端授权后可对话、零原token透传、资源跨角色拒绝，公开+客户holdout k5达门禁；外部“任何harness”仍依支持矩阵发布。

### SP14 — 性能、恢复、运维与正式发行

**输入/前置：** SP12/SP13全部安全门禁，真实双架构Linux。**负责人/审阅：** Release/ops owner；安全、组件与客户reviewer。**顺序：** W01与W02 → W03/W04/W05 → W06发行；监测/备份工具从前期随服务累计，不等此阶段才补。

#### SP14-W01 — 完整F与配额标定

- 执行：固定硬件/行宽/版本，Goose/pgbench/Criterion测50m、CDC5000events/s、20会话query/export、5倍突发和24h创建释放；SLO从源commit到副本可见计。
- 交付/PR：瓶颈/资源图、threshold/config与原始报告；调参只改受审预算，不降低RLS/审计/取消/背压。对比前后同冻结输入。
- 验收/停止：AT-PERF-01/02/03、完整F；p99/吞吐/质量下降/恢复/泄漏目标按Spec。性能差距诚实列明，必要owner书面豁免；安全与正确性不能豁免。

#### SP14-W02 — 完整Compose与health

- 前置/执行：SP11 M1启动；standard/minimal/dev/observability配置隔离、网络default deny/主从/卷/配额/预检；x86_64/aarch64干净VM实际up和重复up。
- 交付/PR：预编译二进制/镜像、支持矩阵、ready检查migration/parser/ACL/replay/audit/Secret/runsc；非TCP探活替代真实能力。
- 验收/停止：AT-OPS-01/02；资源不足/端口冲突/磁盘满/缺runsc/初始失败可重入。dev不标生产、minimal不标物理只读从库。

#### SP14-W03 — 离线与供应链制品

- 前置/执行：W02、已有M1签名；export/import固定digest二进制/镜像/native/字体/SBOM/NOTICE/manifest/hash、公钥；目标离线本地初始化Secret，无网络安装编译。
- 交付/PR：bundle与验签/篡改fixture、双架构离线安装记录；vendor重构建为另一路径，不混为无需编译运行。
- 验收/停止：AT-OPS-03、AT-SEC-05；主密钥/私钥/源密码/token/客户数据扫描0，篡改签名/hash拒绝。禁用许可不进默认包。

#### SP14-W04 — 全状态备份恢复

- 前置/执行：控制/Agent DB/Git/对象/Secret/audit/key registry；备份→故障→fresh恢复→核对lineage与目标/读取checkpoint/可重放历史→追平/必要resnapshot→全量对账。
- 交付/PR：pgBackRest/对象策略、加密主key独立保管、恢复Runbook/实际RPO/RTO；entity key/否定决定/calendar close不可重发或丢失。
- 验收/停止：AT-OPS-06；0差异/守恒/key无复用。恢复历史不足隐藏表并受审resnapshot，不取两个offset最小值机械上线。

#### SP14-W05 — 升级、回滚与应急

- 前置/执行：W04；兼容矩阵和checksum迁移锁、expand/migrate/contract；按控制→Gateway/Entry/MCP/proxy→Loader租约→Debezium→新sandbox升级，PG patch从后主。
- 交付/PR：旧版升级每步失败演练、前向修复/备份恢复、policy保守回滚/轮换/紧急停止；standby初值150s需覆盖export120s余量。
- 验收/停止：AT-OPS-04/05/07；无双写、未确认权限replay不ready，不可逆迁移不能只回滚镜像。生产无批准窗口不自动晋级。

#### SP14-W06 — 观测、渗透与release

- 前置/执行：W01–W05；低基数Prometheus/OTel、安全日志、告警探针和Runbook演练；独立渗透结论，RC锁定digest并收集A–F、私有签名汇总与审批。
- 交付/PR：release manifest/support matrix/notes/证据、告警定位和响应记录；tag仅晋级已验RC同digest，不重新build新包当已验。
- 验收/停止：AT-SEC-06、AT-OPS-08；未处理高危0、日志无正文/Secret；环境/版本/hash不匹配、private gate缺失或二值失败阻断。

**Phase签收：** 双架构fresh/upgrade/offline/recovery、完整A–F、安全/质量/客户gate、签名/SBOM/NOTICE；交付部署者可从manifest复验，不靠作者机器状态。

### SP15 — 可选扩展与下一阶段设计

**输入/前置：** SP14；每扩展独立ADR、feature、支持矩阵。**负责人/审阅：** 扩展owner；相关安全、数据、运维和协议reviewer。**默认：** 全部disabled，不阻M1/M2；选择后按对应执行卡开发，未选写明确未支持，不自动承诺交付全部扩展。

#### SP15-W01 — Kubernetes与其他沙盒驱动

- 执行：以SandboxDriver合同接Helm/PG operator/agent-sandbox/E2B/Kata，NetworkPolicy默认deny；网络/凭证/资源/销毁必须真实目标环境而非trait mock。
- 交付/PR：每driver独立ADR与feature，安装/升级/rollback、identity、S1–S11能力矩阵；不得为外部driver放宽现有接口保证。
- 验收/停止：AT-RT-01/02/04/08与B/C/释放conformance；能力不成立则unsupported。具体供应商未选前只接口/fixture，不引入默认云凭证。

#### SP15-W02 — 新数据库连接器

- 执行：逐SQLServer/仓库及版本定义批量/CDC/删除/cursor/事务/类型能力；复用Connector与真实zero-write、同值/对账/取消suite。
- 交付/PR：每库独立ADR/driver/版本支持矩阵、DBA清单与源读取预算；不能通用驱动名自动涵盖任意SQL库。
- 验收/停止：AT-SRC-01/02/03/05、AT-PL-03/06及适用A；没有真实客户/测试实例的版本不宣布支持。

#### SP15-W03 — Kafka buffer替代

- 执行：先buffer协议adapter，证明事务路由、durable replay、consumer offset/receipt/trim等价；受审迁移checkpoint与可回退窗口，不双consumer重复业务副作用。
- 交付/PR：ADR、独立feature与迁移/rollback Runbook、许可/容量审计；Valkey默认保持，迁移只在已批准窗口。
- 验收/停止：AT-PL-04/05、P11/P15与完整A；缺失/乱序/旧writer或无法证明回放安全阻断切换。

#### SP15-W04 — 指标编译与外部语义格式

- 执行：compile_metric或OSI/dbt/Cube/catalog适配逐一独立协议；schema roundtrip、认证口径/anchor/version/role映射、原始权限不可扩大。
- 交付/PR：格式adapter、feature、semantic diff和D/E同冻结数据比较；外部导入只审核候选，不覆盖curated或自动授权。
- 验收/停止：AT-OOS-01/08/09/11和AT-EVAL-04；roundtrip丢含义或无法表达RLS时明确拒绝/人工映射，不静默降级。

#### SP15-W05 — 列式加速与存储策略

- 执行：独立ADR确认扩展许可/native/parser/PG兼容、列ACL/RLS/只读/函数边界；先相同安全suite再测真实收益，默认feature关闭。
- 交付/PR：扩展构建/镜像、索引/分区计划、迁移/回退与查询差分；不把更快扫描当权限安全证据。
- 验收/停止：AT-DB-01/02/03、AT-GW-01/02/04与AT-PERF-02；任何授权/精度差异阻断，性能收益不显著则不进默认。

#### SP15-W06 — 可复现构建

- 执行：固定vendor/builder/native源码/环境/时钟与产物排序，两个隔离环境独立构建同输入；比较binary/image/layer/SBOM hash并定位非确定段。
- 交付/PR：重构建清单、可复現性报告和离线构建路径；不能仅重用同一cache证明独立重构建。
- 验收/停止：AT-CORE-01、AT-SEC-05；一致或明确标识未可复现部分，不影响已发行制品签名真实性，不虚称bit-reproducible。

#### SP15-W07 — M4独立设计

- 执行：写回/多租户/Oracle/Mongo分别写新spec、威胁模型、审批/tenant隔离/恢复与验证；评审前不进入现有Gateway写入路径。
- 交付/PR：仅设计与兼容影响/迁移方案；新编号与当前只读规范关系明确，默认生产feature不得出现写回。
- 验收/停止：架构检查AT-CORE-01和相关安全review；缺少审批/租户隔离/恢复决策保持设计pending，不能以此卡授权源库业务写。

**Phase签收：** 按每个已选择扩展分别验收；未选不标passed，支持矩阵只能包含真实测试版本。整个SP15不被列为v0.1默认必交能力。

## 5. 测试实施范式与执行入口

### 5.1 从验收描述到真实测试

每个AT维护 `AT ID + scope + test locator + owner + environment + gate + evidence`。locator可以是Rust test、contract fixture、真实e2e或运维演练入口；同一AT跨阶段切片必须分记录，不能最后一个小切片通过就覆盖其他未验部分。

| 测试等级 | 实施方式 | 必须证明 |
|---|---|---|
| unit/property | 无网络、固定seed/可复现反例，纯类型/规则/hash/overlay；错误预期独立于实现 | 边界正确，协议定义一致；不是自算自验 |
| contract | 真实Axum HTTP、SDK/CLI与OpenAPI/schema差分；格式和precision逐字段比 | 一套接口、同权限同精度同warnings，破坏变更可检出 |
| integration | 每run独立PG主从/MySQL/Valkey/Debezium、Toxiproxy | 真授权、事务、水位、receipt、cancel、reject恢复 |
| runtime security | 一次性Linux/gVisor真实沙盒和恶意模型/文件/命令 | 网络矩阵、凭证canary、S1–S11各断点、资源不可残留 |
| model/e2e | 固定snapshot/suite/as_of/模型/harness/预算，逐题独立会话 | 完整回答、引用/警告/诚实和可比统计；不只是工具成功 |
| performance/soak | 规格固定、原始测量、24h与5倍突发，资源/后端可观测 | 端到端耗时、背压、质量与泄漏；不外推小规模 |
| operational | 干净VM、无网络、备份介质、旧二进制与真实升级 | 一键安装、双架构、离线、RPO/RTO、前向修复与止血 |

### 5.2 六轨建立顺序

| 轨道 | 首次交付 | 后续完整验证 |
|---|---|---|
| A 同步正确性 | SP03类型fixture → SP04实际管线A(s100k) | 每PR s；nightly m5m；RC l50m关键场景与故障ledger |
| B 只读/权限 | SP01风险探针 → SP06真实完整B | 每代码PR全部B，主从/minimal均验证；升级与授权变更重跑 |
| C 沙盒与泄漏 | SP01真实runsc探针 → SP10隔离/SP11释放 | runtime敏感PR、nightly、RC与harness/模型变更 |
| D 本体质量 | SP07规则/Pack真值 → SP08业务语义 | SP12全部D指标/盲评/成本，Builder变更受影响重跑 |
| E 问答质量 | SP00数据基础 → SP11真实会话 → SP12完整E | 公开nightly k3、客户发布k3、模型/harness升级k5 |
| F 性能与资源 | 原型和各服务实现随附基准 | SP14完整负载/soak/资源/SLO，RC固定规格 |

A/B/C二值安全必须100%；质量与性能按Spec目标及合法豁免制度，不能取平均掩盖一次越权/泄漏/丢数据。所有case先合法control，预期拒绝必须同时验证错误码与目标无副作用。

### 5.3 必跑故障矩阵

| 故障域 | 注入点 | 断言及证据 |
|---|---|---|
| Pipeline | 事件接收/拒绝耐久/目标COMMIT/控制镜像/XACK/XTRIM/lease断连 | stream/receipt/墓碑/quality/完整applied恢复，停写追平全量0差异 |
| 批量快照 | 部分块后源连接失效，随后源变更；spool写半 | full废旧shadow或完整spool；incremental新run，旧W不推进，无旧/新切面混合发布 |
| 修复 | read replica<P1、读期间CDC/delete、P2未完整应用、lineage改变 | 屏障可证、目标事务重检；不覆盖新数据，不可靠情况block/resnapshot |
| 授权发布 | 角色fence后、目标ACL提交后、从库回放后、CAS前后崩溃 | 旧请求/流/Pack不能看到候选权限，失败封闭，安全恢复后才开放 |
| Gateway | parser失败、慢EXPLAIN/portal/client、late cancel、pool异常、审计磁盘满 | fail closed、wall-clock预算、后端状态可证、无pool串身份，结果前admission耐久 |
| 代理与runtime | echo凭证/DNS/redirect/IP复用/未知RPC/owner失联/S1–S11各强杀 | canary0、出站收紧、5s撤销/回收与60s销毁、重入无孤儿/重复permit |
| 跨层提示注入 | 源注释/业务值/工具输出/feedback/会话混入恶意指令、联网命令与篡改gap | AT-SEC-02在SP07候选写边界、SP10出站/Secret、SP11真实会话、SP13反馈分别验证；定向泄漏0/权限不扩大，受攻击效用单独报告 |
| 文件 | path/zip炸弹/公式/dump、multipart断网、下载后降权 | 隔离进程退出、无宿主副作用/残件/坏产物发布，旧URL5s失效 |
| 运维 | initialize半完成、磁盘满/迁移错、任一步升级、备份恢复/离线篡改 | readiness拒绝、可重入、同digest验签、恢复对账、不可逆显式修复 |

故障注入只在审核测试feature/环境启用，发行制品无用户可触发关闭门禁路径。每run使用唯一run_id与labels，测试结束也执行资源回收；失败后先保留受控证据，再清临时资源，禁止清掉耐久恢复数据而无法诊断。

### 5.4 当前与未来命令的明确区分

当前仓库可执行的文档校验：

```bash
python3 specs/scripts/check_spec.py --self-test
python3 specs/scripts/check_plan.py --self-test
git diff --check
```

SP00负责交付的基础门禁入口（逐项核查实际可用性，不将下列清单视为运行结果）：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo audit
cargo deny check
cargo xtask contracts --check
cargo xtask architecture --check
cargo xtask evidence --check --path evidence/<run-id>
```

真实能力实现后适用的套件入口，参数及命令沿用Spec第18.3章：

```bash
cargo xtask test --suite integration-a --scale s --seed 42
cargo xtask test --suite security-b --database-fallback
cargo xtask test --suite runtime-c --runtime runsc
cargo xtask test --suite release-state --fault-every-step
cargo xtask test --suite operational --profile standard
oo eval run public-smoke --runs-per-item 3
oo eval run customer-dev --runs-per-item 3
oo eval run customer-holdout --aggregate-only --runs-per-item 3
oo reconcile <schema.table> --full
```

未实现suite、缺DB/runsc/授权/客户snapshot返回非0并记录blocked。首个SP00 PR只有实际已存在的模块和测试；CI根据已批准激活清单判断未适用，不用调用不存在的全部套件制造引导死锁，也不把不存在suite报告成功。

### 5.5 示例：如何将一项安全要求变成开发步骤

以下以 Gateway 的角色隔离为例，展示任务拆解方法；实际权限模型、错误码、时限及验收编号仍以 Spec 和对应 SP06 执行卡为准，不新增产品约束。

| 步骤 | 操作与产物 | 验证判据 |
|---|---|---|
| 识别边界 | 绘制请求身份→角色→目标 PG 连接→对象/行权限→审计的调用链，列出绕过入口 | 每个入口能定位到强制授权层；授权不能仅由 prompt 或 UI 决定 |
| 建立独立预期 | 创建两角色及有交集、有隔离的数据；将允许对象、行集合与拒绝理由写入 fixture | 预期由权限契约确定，不读取被测查询输出生成 gold |
| 先验证 control | 用允许角色查询允许数据，再用具备诊断权限的测试账户确认 fixture 存在 | 证明拒绝结果不是数据库未启动、空表或错误路由导致 |
| 建立负例 | 测试禁止表、禁止行、变更角色、连接复用、导出和工具调用路径 | 得到规范拒绝；数据、错误、日志、缓存和导出均不泄漏禁止内容 |
| 实现真实纵切 | 在隔离 PG 主/从库和真实 Gateway 路径执行，保存请求、后端执行与审计关联标识 | 真实授权对象生效；mock 的通过仅作为单元证据 |
| 加入竞态与恢复 | 在查询、流式响应和角色撤销交错处注入故障，验证后端回收与重新授权 | 按 Spec 时限证明旧能力失效；重试不能恢复撤销权限 |
| 证明测试有效 | 在专用测试环境故意配置一个权限缺陷，观察负例失败，再恢复并重跑 | 测试能检出目标缺陷；变异不进入公共服务、发行制品或客户环境 |
| 形成签收包 | 记录 fixture/协议/实现快照、环境、命令、退出码、hash 与审阅结论 | 必须同时具备正确输出、规范拒绝、无泄漏和故障恢复证据 |

其他任务沿用同一方法：先写可观察行为和失败条件，再确定最小真实环境及独立预期，最后增加该能力实际需要的故障。纯模型测试不能签收真实数据库权限，Compose 启动成功不能签收网络隔离，关闭客户端连接不能签收后端查询终止。

## 6. CI/CD、证据与阶段审阅

### 6.1 CI逐能力激活

从SP00交付受保护 `ci/gates.yaml`，记录gate、REQ/AT切片、能力/路径、最迟启用Phase、首次启用commit、runner、required与证据。基础构建/格式、Secret和依赖安全从首个代码PR适用；尚未建立完整套件明确 `not_applicable_until_phase`，不是passed。引入Pipeline/Gateway/runtime能力的同PR必须激活真实对应测试；SP04/06/10–11分别完整A/B/C后永久required。

聚合job比对受保护基线、PR路径/能力、实际job与Phase退出证据；阻断缺job、unknown skip、已required降级、逾期未激活。PR不能改自己的清单规避检查。删除能力只能正式需求/支持矩阵变更与独立审阅；普通skip不构成批准。

| 触发 | 固定执行规则 |
|---|---|
| docs-only | 来源/需求/Plan覆盖/链接/围栏校验，独立review |
| 代码PR | 基础+contracts/architecture/supply-chain+所有已激活A(s)/B；相关路径加C/D/E |
| runtime/安全 | 一次性runsc runner的C与release-state故障，不能仅mock |
| Builder/语义 | 受影响D、G正负fixture、可用公开E；客户Ontology发布另有私有G10 |
| nightly | A(m5m)、C、公开E k3、轻量soak、依赖扫描与可比基线 |
| 模型/harness/沙盒升级 | C、公开E、客户holdout k5，同snapshot条件比较 |
| M1发行 | M1全部适用gate及客户UAT；签名/SBOM/NOTICE从此时开始 |
| 完整RC | 双架构构建、扫描、签名、全部A–F、fresh/upgrade/offline/recovery、私有签名汇总 |
| tag/晋级 | 核对已验RC digest与approval，只推广同digest，不重建新包 |

fork PR无客户/生产Secret和高权限OIDC；native/build.rs/proc-macro无Secret隔离builder；runsc runner一次性。不可信PR不在长期敏感自托管节点运行，不把pull_request_target与PR执行代码混为高权限工作流。

模型/客户数据缺失按pending/blocked，不用无限等待伪绿或忽略required。初始无7日质量基线时建立冻结bootstrap baseline、初始目标与人工质量审阅；7日后启滑窗，不能null当100%。公开执行只合成数据，私有仅客户批准runner与驻留端点。

### 6.2 Evidence结构与保存

默认目录为 `evidence/<run-id>/<phase>/<task-id>/`，Phase根manifest汇总引用，客户私有证据存在独立私有根，不进入公共Git。Evidence schema沿用Spec第20.3章；公开证据默认1年、私有按客户retention。

每个任务签收包至少包含：

- Spec/Plan版本、实现commit、Task/REQ/AT与scope、实际前置证据引用。
- dataset/snapshot/suite/scorer、policy/knowledge/config/type/hook/schema、模型/harness和制品digest完整适用版本元组。
- 环境规格、依赖真实版本、运行命令、开始/结束、退出码、原始结果与sha256。
- 正例、负例、故障点ledger、目标/后端/容器资源状态与关键指标。
- reviewer身份、结论、时间、待修/限制；合法质量性能豁免的owner/期限/监测/任务。

passed必须实际exit0、artifact存在且hash匹配、环境/scope齐备。pending/blocked/skipped/incomplete不得折算passed；截图和CI URL只是补充。客户release gate用可信签名的汇总pass/fail+版本hash，不上传题目/gold/SQL/trace；聚合器验证报告针对本次构建而非旧版本。

### 6.3 Phase审阅会议的固定议程

1. 对照Spec工作项清单逐项核查执行卡、未达项和适用AT；没有实现的功能保持planned/blocked。
2. 复验F冻结契约、源/权限/网络/凭证/资源边界；选择至少一个关键负例和一个故障恢复现场重跑。
3. 审核证据hash、版本、输入与CI激活状态；严查切片被冒充完整、旧报告复用和未知skip。
4. 审核性能/质量和剩余风险，确认问题有owner/期限；二值安全与必要正确性失败阻止退出。
5. 签署Phase manifest，发布给下一Phase的输入契约与限制；客户签收只由客户授权角色确认。

### 6.4 发布和生产操作

主分支合并可以自动构建、扫描和部署合成staging。客户生产晋级前必须有具体RC、升级diff、备份、迁移兼容、回滚/前向修复及最新证据，按客户预授权窗口或发布owner执行，不把本Plan视为生产操作批准。

发布顺序：验签/manifest→preflight→安全准入保护→迁移/滚动升级→health/B/C/smoke→权限与副本确认→解除保护。失败停止下一步并保持安全封闭，不自动恢复已撤销权限。不可逆数据/schema变更必须备份恢复或前向修复；源端永不业务写回。

## 7. 首批执行与交接清单

### 7.1 从当前仓库开始

第一批仅启动SP00，不创建全部业务服务的假实现。执行顺序固定为：

1. 文档完整性检查，记录实际Cargo workspace、已有实现、验证结果及用户已有文件；创建或更新任务/owner/reviewer台账。
2. SP00-W06开源/安全贡献基线和SP00-W01工具链/真实依赖核验；将候选不兼容登记为明确阻断。
3. 冻结oo-core最小协议，完成W02/W03合法与负例；避免为将来功能一次性造巨大对象模型。
4. W04建立xs/s和授权/攻击fixture；W05交付xtask、服务fixture、Evidence和单调gate。
5. 干净clone重新运行已适用门禁，人工变异证明关键校验能失败，签收SP00。
6. 领取SP01 P项和SP02不依赖风险结果的任务，按具体证据解除后续阻断。

### 7.2 每次开发结束的交接格式

```text
任务与状态：<ID> / verified或blocked，验收scope
实际变更：行为、接口、迁移、权限/依赖变化
验证：命令、退出码、原始报告和hash，不写“应该通过”
安全与资源：源审计、ACL/网络、取消/销毁状态，适用时逐项给证据
未达与限制：影响、owner、解除条件，客户pending单独列
交付输入：下一任务使用的契约版本、fixture、artifact与前置证据
审阅：独立reviewer结论与签收记录
```

### 7.3 本Plan文档的验收

本次文档交付需验证：16个Phase各一次、87个工作项逐卡覆盖且不新增未知编号、P1–P16每项有执行和停止判断、全部96个验收场景有执行引用且每卡覆盖原工作项验收、阶段签收和统一工作协议完整、本地链接/Markdown围栏有效、Spec与docs基线未漂移。

[文档检查脚本](scripts/check_plan.py)只核查上述结构和追踪完整性，不证明产品、外部依赖、Docker、SQL算法、模型、CI或性能运行通过。其self-test在内存中人为删除任务/Phase/原型、删除或篡改验收、引入坏链接/围栏以证明能拒绝，绝不修改正式文档。产品验证仍由对应Phase真实门禁完成。

### 7.4 开工检查表与停止条件

开发者领取任何工作项时填写下表，附到任务卡或 PR；“未知”必须写明调查步骤，不能作为满足前置条件。

| 检查项 | 必填记录 | 不满足时的处理 |
|---|---|---|
| 要求与范围 | Task、REQ、AT切片、Spec版本及禁止修改区 | 先核对执行卡和来源；不从现有代码反推需求 |
| 前置证据 | 契约版本、原型结论、依赖签收记录 | 可做独立原型；不得启用依赖未验证方案的默认能力 |
| 写入分配 | 目录 owner、共享文件 owner、并行任务冲突检查 | 先更新台账、串行安排共享修改 |
| 数据与资源 | 合成/私有分类、环境、预算、资源 owner、清理方法 | 缺授权时只使用合成数据；缺环境保持 blocked |
| 测试预期 | 合法 control、负例结果、故障点和资源终态 | 先建立能失败的验证；不以任意错误代替规范拒绝 |
| 交付与审阅 | 产物位置、验收命令、独立 reviewer、证据目录 | 审阅人待定可推进实现，但不得自行签收安全变更 |
| 集成影响 | 契约差分、迁移兼容、CI能力激活和回滚策略 | 与能力启用同 PR 补齐必要门禁和保护 |

任务实现后的检查顺序固定为：核对写入范围→执行适用验证→检查资源终态→生成证据清单→独立审阅→更新状态。存在安全失败、精度损失、版本不匹配、证据缺失或未实现的 required suite 时，保持未签收；文档检查通过不改变这些状态。

开发状态补充：a111固定7acca207独立复审批准CLI-FIFO-OPEN-BLOCKS，仅该修复切片勾选；完整CLI query/SP09仍未验收。a107 jobs复审继续；a109基础许可证记录复审接替已释放槽B。以开发台账及固定提交证据为准，完整87工作项和16Phase未完成。
