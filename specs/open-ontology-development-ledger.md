# Open Ontology 开发任务清单与并行工程表

| 属性 | 值 |
|---|---|
| 基线 | OO-SPEC-001 v1.0.0、OO-PLAN-001 v1.0.0、docs全部19份 |
| 执行模型 | 所有编码、自动化、测试实现、E2E与外部审阅：Grok Build `grok-4.7` / `xhigh` |
| 协调与终验 | 当前Codex：任务冻结、分区、进程管理、证据核验、最终review；不代写产品代码 |
| 产品推进规则 | 用户于2026-10-02更新：基础修复单独工作线；优先PG→导入/catalog→可审核ontology草案→受控查询→CLI对话，随后扩库/文件/CDC/完整harness；小功能先Linux/MSRV局部冒烟，固定提交复审，集成节点全矩阵 |
| 工作区规则 | 用户最新建议不开tree：不再新建worktree；现有product-pg用于完成已派任务固定提交复审和该功能必要返工，随后按明确源范围整合回主工作区，后续单工作区分区/串行共享依赖 |
| 提交与验收 | 已获本地提交授权；交付后立即明确git add并commit，不push/发布；复审绑定固定SHA。开发检查与负责人/客户/保护CI/runsc等发行验收分开，不阻塞无关公共功能 |
| 速度指标 | 记录真实已交付功能数、完成验收工作项数、返工任务/已交付任务；不以代码行数或review轮数计进度 |
| 初始状态 | 只有docs/specs，无产品Cargo workspace；所有产品工作未完成 |
| 等待策略 | 用户于2026-10-02更新：确认真实进程handle；收到退出结果后立即核对产物和证据、判断下一步；运行中按任务阶段与预计耗时安排等待，不固定30分钟，不持续或密集轮询，不因安静或观察超时重启 |
| 完成定义 | 所有适用工作真实完成，AT/不变量/原型/构建/完整测试/E2E/审阅/客户门禁证据齐备；不缩小为骨架或小样通过 |

## 1. 写入边界与冲突规约

每个Grok执行者有独立提示词、CLI session UUID、stdout日志、任务范围与证据目录。禁止启动Codex原生子Agent或嵌套Codex；Grok也设置`--no-subagents`，并行由协调者直接调度多个独立Grok进程。

初始波次在同一工作区执行，但写入范围严格分离。Cargo根manifest、lock、toolchain、共享oo-core和root CI一次只允许一个writer。消费者只读已冻结契约；要改共享接口先报协调者，串行Grok integration pass修改并回归，其他writer等待。

任何执行者不得修改docs、Spec、Plan、来源哈希、其他执行者文件、用户`.DS_Store`/`.specstory/`，不得通过删要求、改gold、弱化门禁或mock代替实际安全验收。需变更设计只提交具体失败证据与ADR建议，由协调者冻结后再授权实施。

禁止git reset/clean、删除用户文件、push/PR/部署/发布、读取认证存储或真实客户Secret、操作真实源数据库。本地commit已获用户授权，交付后明确文件范围提交并绑定独立审阅SHA，不能把commit当验收通过。网络查询限必要官方依赖/协议资料；公共开发使用合成fixture。独立reviewer只读，修正仍由Grok实现者完成。

## 2. Phase工程表

角色名称是可复用的执行槽，每次派工绑定新的明确session和文件范围；不是允许同角色无限修改全部仓库。所有Phase的退出门禁仍以Spec/Plan为准。

| Phase | Grok执行槽 / 工作 | 前置 | 可并行范围 | 写入owner / 冻结接口 | 独立验收 |
|---|---|---|---|---|---|
| SP00 | FOUNDATION W01；GOVERNANCE W06；CORE W02/W03；DATAGEN W04；HARNESS W05；ENV测试基础子项 | 无；W02–W05依基础契约 | 第一波W01+W06；W01完成后CORE；CORE冻结后DATAGEN+HARNESS；ENV只做独立测试基础/冻结快照构建 | FOUNDATION独占root Cargo/lock；CORE独占oo-core；DATAGEN仅datagen/fixture；HARNESS仅xtask/tests/CI，经授权整合root；ENV仅testing/infra与自己的Docker测试资源 | Grok FOUNDATION-REVIEW+Codex实测 |
| SP01 | PIPE-SPIKE P1–P4/P11/P12/P15；QUERY-SPIKE P5–P7/P13/P14；RUNTIME-SPIKE P8/P9/P16；SEMANTIC-SPIKE P10 | SP00薄原型 | 独立spike目录，资源run_id隔离 | 无人修改产品共享契约；原型结论冻结后integration | 独立Grok风险review+Codex原始证据 |
| SP02 | CONTROL W01–W03；IDENTITY W04/W05；COLLECTOR W06 | SP00；对应P门禁 | 先共同模型，随后分模块 | CONTROL迁移/jobs；IDENTITY鉴权/Secret；COLLECTOR受限接收；共享types由integration写 | 真PG/并发/轮换/RBAC/恢复 |
| SP03 | CONNECTOR-PG W01；CONNECTOR-MYSQL W02；SOURCE-CORE W03–W05 | SP02、P2/P14/DEC-17 | Connector/type/position冻结后两个driver独立 | SOURCE-CORE共享trait；driver不改trait/root | 双真实驱动、源审计零写 |
| SP04 | PIPE-WRITER W02/W05；CDC W01/W04；SNAPSHOT W03/W06 | SP03、P1–P4/P11/P15 | receipt/fence/Batch冻结后不同子模块 | 单目标写者、跨库receipt/墓碑；共享迁移单writer | 完整A、强杀ledger、全量0差异 |
| SP05 | FILE-STORE W01/W06；FILE-PARSE W02–W04；FILE-IMPORT W05 | SP02/03、Loader、P16 | 对象/IPC合同冻结后解析器独立 | 文件/对象生命周期单owner；import复用Loader | 全格式真fixture、local+S3、恶意文件 |
| SP06 | DB-POLICY W01；SQL-GUARD W02；GATEWAY W03–W05；AUDIT W06 | SP02/03、P5–P7/P13/P14 | policy/catalog/审计契约后分模块 | 无Gateway写库/connector依赖；root变更integration | 完整B、真PG主从、取消/审计 |
| SP07 | OOS W01/W05/W06；PROFILE W02/W03；BUILDER-LLM W04 | SP02–06真实数据 | OOS/证据/Pack合同后分模块 | generated/curated边界、角色Pack独立 | D组件、PII/Pack/审核负例 |
| SP08 | ENUM-CALENDAR W01；METRIC W02；ENTITY W03；PUBLISH W04；REPAIR W05；CONSOLE W06 | SP04/06/07 | schema/鉴权/owner合同先冻结 | 发布fence单owner；派生资产/迁移integration | G1–G13、control_total、守恒、恢复 |
| SP09 | SDK W01/W03；CLI W02/W05；TS-SDK W04 | SP06/08 | HTTP/export completion冻结后 | SDK/CLI/TS各独立；禁止驱动直连 | HTTP/Rust/TS/CLI差分、坏流 |
| SP10 | SANDBOX W01/W02/W05；PROXY W03；PI W04 | SP02/06/09、P8/P9/P16 | 网络/身份/事件合同后 | 容器API与proxy身份分区 | 真Linux/runsc C与canary0 |
| SP11 | SESSION W01/W02/W03；WEB W04/W06；FEISHU W05 | SP08/10 | session/events/S1–S11冻结后 | 释放状态与query reaper单owner；UI无DB权限 | 全旅程、5s撤销/60s销毁/重启 |
| SP12 | BENCH W01–W03；CUSTOMER W04/W05；UAT W06 | SP04–11、客户授权输入 | 公开与私有runner隔离 | holdout独立管理员；不污染dev | 300题、A–F适用、私有gate、UAT |
| SP13 | MCP W01/W02/W03；FEEDBACK W04；ADAPTER W05 | SP09/11/12、rmcp证据 | OAuth/resource合同后 | audience/token exchange专属owner | MCP真客户端、C/E k5 |
| SP14 | PERF W01；OPS W02–W05；RELEASE W06 | SP12/13、安全全部 | 各run_id资源隔离，晋级串行 | digest/migration/升级锁单owner | 双架构/离线/恢复/完整A–F/渗透 |
| SP15 | EXTENSION W01–W07 | SP14+逐项ADR | 每feature独立，默认关闭 | 每扩展独立支持矩阵，不变安全边界 | 真环境conformance；M4仅设计 |

依赖波次：SP00→SP01/SP02→SP03→SP04/SP05/SP06→SP07→SP08→SP09→SP10→SP11→SP12→SP13→SP14→SP15。可以提前做不依赖后续能力的fixture/原型，但不能提前签收完整Phase。完整范围保持不变；客户资料、真实Linux/runsc、50m规模、外部客户端未提供时标具体blocked，不伪造完成。

## 3. 全量To-Do

勾选仅在Grok独立review与Codex核验证据后进行；代码存在或执行者自称通过不能勾选。子项拆分沿用父ID，未勾选部分继续推进。

### SP00

- [ ] SP00-W01 Cargo/toolchain/lock/features/架构规则
  - [x] W01架构检查器两轮缺陷修复：a014独立复审批准切片，Codex当前workspace复验通过；完整W01未完成。
- [ ] SP00-W02 配置/OOS/Batch/API/Error schema
  - [x] CORE公开入口修复切片：a036独立复审approve，273探针零失败、62crate tests及Clippy通过；纯模型F0可消费，完整API/驱动/平台/AT仍未签收。
- [ ] SP00-W03 类型/规范值/hash/脏值fixture
- [ ] SP00-W04 Rust xs/s数据生成与攻击/角色fixture
  - [x] DG01–08修复切片：a035独立复审approve，192独立checks零失败；完整当前源码集成/平台门禁待补，父项仍未签收。
- [ ] SP00-W05 xtask/Compose/CI/Evidence/激活门禁
  - [x] ENV bootstrap/日志/health拒绝修复切片：a038独立复审批准，34文件hash匹配a034；实际fixture证据有效，当前四构建和完整W05未签收。
  - [ ] W05 early-setsid/父进程立即退出的执行归属与输出清理：a062固定f61复审request_changes，a070专用基础修复线；未知containment不能签required_satisfied。
- [ ] SP00-W06 开源/安全/供应链规范
  - [x] W06治理record-shape QA修复：a014独立复审批准切片；完整制品审计和真实负责人仍待完成。
  - [x] W06 ISC规则/归属记录修正切片：a069规则独立复核、a078批准fb2d1d1正文/erratum；OR策略差异、秘密历史暴露与完整供应链仍未签。
- [ ] SP00 Phase退出与独立审阅

### SP01

- [ ] P1 CDC-first快照/fill/delete/50m
- [ ] P2 位点/事务/epoch/source零写
- [ ] P3 Debezium offset/history恢复
- [ ] P4 Valkey AOF/背压/gap
- [ ] P5 parser/native/SQL差分
  - [ ] P5-a PG17 parser匹配/受限IPC/语句拒绝与star差分：a065交付efb61d6，a076固定复审；作者已记录R9 WHEREtrue/子查询绕过及未实现R4，完整R1–R9与DB绑定另验。
- [ ] P6 PG claims/ROLE/RLS/pool
- [ ] P7 真50m EXPLAIN/资源阈值
- [ ] P8 真runsc网络/身份隔离
- [ ] P9 Pi真实RPC/abort/restore
- [ ] P10 客户初稿质量/驻留基线
- [ ] P11 完整事务流/过滤计数/idle进度
- [ ] P12 大事务分段/暂停/质量
- [ ] P13 列授权/掩码/raw/实体绕过
- [ ] P14 驱动/精度/取消/双架构native
- [ ] P15 租约/目标fence/COMMIT/ACK/TRIM恢复
- [ ] P16 native强杀/释放/部署/协议
- [ ] SP01风险决策/ADR/Phase退出

### SP02

- [ ] SP02-W01 控制迁移/jobs/lease/schedule
  - [x] SP02-W01-a 真实控制库迁移/专用角色/约束：70a4e72获a068固定复审approve；协调者核验完整报告、117探针全ok/0失败、Linux1.94测试/Clippy0和三个文件hash。仅schema切片，driver/100worker/备份与完整AT仍未签。
  - [ ] SP02-W01-b Rust队列领取/续租/generation/schedule/retry：待迁移固定接口与依赖整合。
  - [x] SP02-W01-c event项目归属约束：9224d31获a079固定复审approve；真实LOGIN跨项目23503、坏历史及并发坏历史OO004回滚、合法历史不变、Linux1.94局部测试/Clippy0。仅项目绑定，不签事件内容真实性或完整驱动。
  - [ ] SP02后续适用验收：applicator哈希校验与readiness合取、未来函数必须以owner创建而非adopt未审definer、运行时CAS/TTL/最小项目scope；a068-F1/F3/F4/F5不得因schema子项approve而忽略。
- [ ] SP02-W02 config/Git/CAS
- [ ] SP02-W03 validate/plan/apply恢复
- [ ] SP02-W04 Secret/轮换/mTLS
- [ ] SP02-W05 OIDC/bootstrap/RBAC
- [ ] SP02-W06 Collector/反馈受限receiver
- [ ] SP02真实集成/Phase退出

### SP03

- [ ] SP03-W01 PG connector/体检/DBA清单
  - [ ] V01-PG-01 源连接/AccessReport/catalog：4ecdbf7三HIGH修正获a074批准；main整合2980e85经a087复审发现服务端role GUC导致TLS元数据误判，a093修正中；旧b2注册结果不得用于导入准入。
  - [x] V01-PG-01-F 三HIGH安全纠正切片：a074固定4ecdbf7独立权限/env/TLS及零写验证批准；仅当前图，不签完整connector或main集成。
- [ ] SP03-W02 MySQL connector/体检
- [ ] SP03-W03 chunk/cursor/bounded stream/cancel
- [ ] SP03-W04 type/PositionCodec/精度
  - [ ] W04-a PG无损真实SQLx解码/Arrow：a098实际启动；仅connector代码/隔离快照，主依赖等待a095明确交接。
- [ ] SP03-W05 catalog/名字映射/注册
- [ ] SP03源审计/Phase退出

### SP04

- [ ] SP04-W01 Debezium/decoder/事务流
- [ ] SP04-W02 COPY/merge/TOAST/墓碑
- [ ] SP04-W03 L0/chunk/fill/shadow
- [ ] SP04-W04 大事务spool/隔离/schema
- [ ] SP04-W05 fence/receipt/reject/ACK/TRIM
  - [ ] W05-a 目标锁/generation与耐久receipt迁移：8370498经a085 request_changes；NULL分段混合可入库，a092前向迁移修正中。Loader driver、控制镜像和ACK/TRIM另验。
- [ ] SP04-W06 full/incremental/快照失效恢复
- [ ] SP04完整A/Phase退出

### SP05

- [ ] SP05-W01 multipart/checksum/local/S3/ACL
- [ ] SP05-W02 CSV/TSV/JSONL/Parquet
- [ ] SP05-W03 xls/xlsx/日期/公式
- [ ] SP05-W04 SQLite/dump隔离恢复
- [ ] SP05-W05 import策略/lineage/取消
- [ ] SP05-W06 下载/保留/删除/reaper
- [ ] SP05全格式/Phase退出

### SP06

- [ ] SP06-W01 ACL/RLS/helper/drift
- [ ] SP06-W02 parser/R1–R9/star/OID
- [ ] SP06-W03 token/mTLS/撤销/准入
- [ ] SP06-W04 RR/portal/cancel/pool/reaper
- [ ] SP06-W05 typed结果/质量/freshness
- [ ] SP06-W06 audit WAL/completion/read-own
- [ ] SP06完整B/Phase退出

### SP07

- [ ] SP07-W01 OOS/overlay/stale
  - [ ] V01-OOS-01A 确定性catalog结构草案/审核队列/版本绑定：a071五项返工；closure eebc0ea由a086原会话续审，NFC a088原会话续接，均未签收。
  - [x] V01-OOS-01A-C findings3/4端点闭包与重复key修正：a086固定eebc0ea独立118cases/0failure批准；仅此修复切片，NFC与完整草案另验。
  - [x] V01-OOS-01A开发预检：a063首次Linux/MSRV冒烟已核验attempt-2原始日志，rustc1.94.0、62个core测试、fetch/test/Clippy exit0；attempt-1的101保留。此项仅证明开发基线，不证明草案功能或SP07验收。
- [ ] SP07-W02 catalog/profile/evidence
- [ ] SP07-W03 规则/关系/码表/专家卡
- [ ] SP07-W04 LLM候选/驻留/预算
- [ ] SP07-W05 审核/owner/分类
- [ ] SP07-W06 role Pack/缓存/manifest
- [ ] SP07 D组件/Phase退出

### SP08

- [ ] SP08-W01 enums/calendar/结账
- [ ] SP08-W02 metric/control_total/diff
- [ ] SP08-W03 entity/key/alias/否定/守恒
- [ ] SP08-W04 G1–G13/publish/fence/replay/CAS
- [ ] SP08-W05 reconcile/hook/repair
- [ ] SP08-W06 管理/审核/发布UI
- [ ] SP08语义发布集成/Phase退出

### SP09

- [ ] SP09-W01 Rust SDK/统一HTTP
  - [ ] W01-a 有界JSON query Rust SDK：a084发现六项runtime问题；bde2e8c修正交付，a090固定复审中；供应链独立pending。该协议验收不替代生产Gateway/PG闭环。
- [ ] SP09-W02 CLI/认证/输出/退出码
  - [ ] W02-a 外部认证query/JSON/Markdown/退出码：a095实际启动；沙盒proxy/管理命令/完整对话另验。
- [ ] SP09-W03 Arrow/EOS/Parquet/completion
- [ ] SP09-W04 TS SDK/contracts
- [ ] SP09-W05 oo-analysis/中文图表/安全文件
- [ ] SP09完整契约差分/Phase退出

### SP10

- [ ] SP10-W01 SandboxDriver/受限Docker/runsc
- [ ] SP10-W02 专属网络/UDS/IP generation
- [ ] SP10-W03 proxy/路由/Secret/预算
- [ ] SP10-W04 Pi/RPC/events/abort/restore
- [ ] SP10-W05 镜像/Pack/工具/预热
- [ ] SP10真实C/Phase退出

### SP11

- [ ] SP11-W01 session/turn/identity/CAS
- [ ] SP11-W02 S1–S11/reaper/资源闭环
- [ ] SP11-W03 outputs/transcript/restore
- [ ] SP11-W04 网页chat/events/引用/安全渲染
- [ ] SP11-W05 飞书签名/身份/私聊
- [ ] SP11-W06 运维/停止UI/M1 Compose
- [ ] SP11全旅程/Phase退出

### SP12

- [ ] SP12-W01 300题/dataset/gold
- [ ] SP12-W02 snapshot/独立eval会话/as_of
- [ ] SP12-W03 scorer/judge/统计/报告
- [ ] SP12-W04 客户接入/驻留/DBA/部署
- [ ] SP12-W05 dev/holdout/live/私有runner
- [ ] SP12-W06 真实UAT/MVP签收
- [ ] SP12客户G10/M1完整门禁

### SP13

- [ ] SP13-W01 MCP transport/resources/tools
- [ ] SP13-W02 OAuth/PKCE/audience/exchange
- [ ] SP13-W03 token UI/role约束
- [ ] SP13-W04 S6–S8/feedback候选
- [ ] SP13-W05 已选择harness/渠道独立适配
- [ ] SP13真客户端/C/E k5/Phase退出

### SP14

- [ ] SP14-W01 完整F/50m/20session/soak
- [ ] SP14-W02 Compose全profile/双架构
- [ ] SP14-W03 离线bundle/验签/SBOM
- [ ] SP14-W04 全状态backup/recovery/RPO/RTO
- [ ] SP14-W05 migration/升级/回滚/停止
- [ ] SP14-W06 观测/渗透/签名RC/release
- [ ] SP14完整A–F/双架构/离线/恢复/客户门禁

### SP15

- [ ] SP15-W01 已选择K8s/沙盒驱动扩展
- [ ] SP15-W02 已选择新库connector扩展
- [ ] SP15-W03 Kafka buffer独立交付包
- [ ] SP15-W04 metric/外部语义格式独立包
- [ ] SP15-W05 列式/存储策略独立包
- [ ] SP15-W06 可复现构建
- [ ] SP15-W07 M4写回/多租户/Oracle/Mongo独立设计
- [ ] SP15逐项ADR/默认关闭/conformance/Phase状态

SP15扩展按Spec保持默认关闭和逐项独立ADR。本目标要求全计划推进：交付各工作项规范要求的独立包/设计及证据；未获得具体供应商或目标实例时明确记录依赖，不能把“不默认启用”偷换为“无需开发”。

## 4. 派工记录

| 波次 / 槽 | CLI session | 任务 | 写入范围 | 状态/证据 |
|---|---|---|---|---|
| 00A / FOUNDATION | `eef831aa-dba4-4af2-a78b-541bc9e0a001` | SP00-W01 | root Cargo/toolchain/lock/.gitignore、各crate最小骨架、dependency-baseline；不实现oo-core业务类型 | CLI退出0，in_review；Codex fmt/clippy/test及架构negative复验通过（11tests）；Linux双架构/native/audit未验；证据`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a001/SP00/SP00-W01/` |
| 00A / GOVERNANCE | `eef831aa-dba4-4af2-a78b-541bc9e0a002` | SP00-W06 | LICENSE/NOTICE/README/贡献与安全模板、CODEOWNERS、ADR模板、供应链政策；不写Cargo/代码/CI工作流 | CLI退出0，in_review未签收；handle `43537`；证据`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a002/SP00/SP00-W06/`；真实负责人/联系渠道待输入 |
| 00A-R / GOVERNANCE-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a003` | SP00-W06独立审阅 | 只读治理文件/证据，禁止修改 | CLI退出0；中等级许可证/年龄校验缺陷及低等级表单/skip/归属问题；原始review `/tmp/open-ontology-grok-00a-governance-review.log`；待修不签收 |
| 00A-R / FOUNDATION-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a004` | SP00-W01独立审阅 | 只读架构检查器/已存证据；CORE同时添加模型，审阅区分基线和增量 | CLI退出0；required边未区分normal/dev/build/optional/target，测试存在盲区；review `/tmp/open-ontology-grok-00a-foundation-review.log`；待修 |
| 00B / CORE | `eef831aa-dba4-4af2-a78b-541bc9e0a005` | SP00-W02/W03 | oo-core、契约/规范值fixture、schema/OpenAPI生成物、root依赖/lock单writer、增量依赖记录；不改oo-arch-check | CLI退出0，in_review；Codex当前workspace测试/格式/Clippy退出0：core27+arch43；其余业务crate仍骨架；已知字节上限/日期/时区/依赖许可待独立审阅 |
| 00A-F / FOUNDATION-FIX | `eef831aa-dba4-4af2-a78b-541bc9e0a006` | SP00-W01审阅修复 | 仅oo-arch-check及新修复证据；不改root Cargo/lock/oo-core/既有证据 | CLI退出0，in_review；Codex复验27 tests及Clippy通过；完整W01未签收；提示词`execution/prompts/00a-foundation-fix.md` |
| 00A-F / GOVERNANCE-FIX | `eef831aa-dba4-4af2-a78b-541bc9e0a007` | SP00-W06审阅修复 | 治理/模板/新治理校验器和新证据；不改Cargo/crates/旧证据 | CLI退出0，in_review；Codex复验109适用checks、49controls，2个明确skip；完整AT-SEC-05未签收；提示词`execution/prompts/00a-governance-fix.md` |
| 00B / ENV | `eef831aa-dba4-4af2-a78b-541bc9e0a008` | SP00-W05测试基础子项、W01双架构补验 | 仅testing/infra、新证据、独立带run_id labels的Docker测试资源；不改Cargo/core/xtask/root CI | CLI退出0，in_review；冻结旧CORE快照4组build/test通过但Clippy退出101；fixture smoke退出0；Toxiproxy未接库、runsc未注册；完整W05未签收 |
| 00A-R2 / CORRECTIONS-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a009` | W01/W06修复独立复审 | 只读架构检查器与治理代码；仅可写本次review证据目录；不读移动CORE为冻结事实 | CLI退出0，request_changes；年龄calendar-day中等级缺陷、2架构与3治理低等级缺口；证据`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a009/SP00/corrections-review/` |
| 00A-F2 / ARCHITECTURE-HARDENING | `eef831aa-dba4-4af2-a78b-541bc9e0a010` | W01两个边界修复 | oo-arch-check源码、修复handoff、新a010证据；不改manifest/lock/CORE | CLI退出0，in_review；43tests，前修10个新失败已存证；Codexworkspace复验通过；待复审 |
| 00A-F2 / GOVERNANCE-HARDENING | `eef831aa-dba4-4af2-a78b-541bc9e0a011` | W06真实168h年龄/许可证定位/否定修复 | 治理QA、policy、依赖form、空模板、新a011证据；不改crate/旧证据 | CLI退出0，in_review；真实UTC elapsed与120 controls；Codexself-test退出0；完整ATSEC05与独立复审待补 |
| 00B-R / ENV-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a012` | 测试基础与快照审阅 | 只读infra/旧证据；仅新a012review证据；不运行资源修改脚本 | CLI退出0，request_changes；6项快照/凭证日志/脚本/误判/资源归属缺陷；review证据已存a012 |
| 00B-R / CORE-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a013` | W02/W03契约审阅冻结 | 只读oo-core/contracts/examples/依赖增量；仅新a013证据 | CLI退出0，request_changes；F1–F15字节/日期/时区/规范值/精度/许可证等；接口不冻结，不启动消费者 |
| 00A-R3 / HARDENING-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a014` | W01/W06第二次修复复审 | 只读架构/治理；仅新a014证据 | CLI退出0，approve修复切片；43tests+39架构探针、120QAcontrols+107治理探针；完整任务仍不签收 |
| 00B-F / ENV-HARDENING | `eef831aa-dba4-4af2-a78b-541bc9e0a015` | W05测试基础6项修复及故障fixture | 仅testing/infra、新a015证据、自己的UUID Docker资源；root/core冻结 | CLI退出0，in_review；两独立fixture smoke、6代理故障恢复、隔离cleanup证据；4组构建快照含CORE修复红测试，实际test/Clippy全部101，未签收 |
| 00B-F / CORE-HARDENING | `eef831aa-dba4-4af2-a78b-541bc9e0a016` | W02/W03契约修复F1–F15 | oo-core/contracts/config示例、root依赖/lock唯一writer、增量许可、新a016证据 | CLI退出0，in_review；Codex fmt/workspaceClippy/core47tests退出0；含19冻结回归；root/core再次冻结 |
| 00B-R2 / ENV-HARDENING-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a017` | ENV 6项修复独立复审 | 只读infra/a015；仅a017证据；无真实资源操作 | CLI退出0，request_changes；4项剩余拒绝缺陷：冻结脚本symlink、隐藏secret、..extra清理、DNS无归属删除 |
| 00B-R2 / CORE-CORRECTION-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a018` | CORE F1–F15复审/冻结 | 只读core/contracts/许可；仅a018review证据 | CLI退出0，request_changes；199独立checks通过1失败，另公开入口归一化/内存对象/Arrow验证缺口；DATAGEN可用已批准切片，完整W02/W03未签收 |
| 00B-F2 / ENV-FINAL-CONTROLS | `eef831aa-dba4-4af2-a78b-541bc9e0a019` | ENV剩余4项修复/当前源码4构建 | 仅infra/a019证据/自己的UUID资源；root/core只读 | handle1323终态0但报告缺失，fixture driver退出1；需恢复明确session完成，退出0不证明完成 |
| 00B-F3 / CORE-BOUNDARIES | `eef831aa-dba4-4af2-a78b-541bc9e0a020` | W02/W03剩余4个边界 | oo-core/contracts/a020证据；不改root/其他模块 | CLI终态0，in_review；DISPOSITION及红绿证据已存在；作者报告core55tests，待独立复验；完整W02/W03未签收 |
| 00C / DATAGEN | `eef831aa-dba4-4af2-a78b-541bc9e0a021` | SP00-W04 | bench/datagen独立Rustworkspace/bench索引/a021证据；root依赖与core只读 | CLI终态0，in_review；report/32文件hash与xs/s实跑证据存在；作者14tests，待独立审阅；完整AT-EVAL-01未签收 |
| 00B-F2-R / ENV-RESUME | `eef831aa-dba4-4af2-a78b-541bc9e0a019`（明确resume） | 完成未交付fixture/4构建/报告 | 原a019范围，已有证据不覆盖；补新attempt | CLI退出0，in_review；报告补齐，119文件快照4组build/test90/Clippy全0；首attempt隔离失败1保留，第二fixture driver0；runsc仍缺失 |
| 00B-R3 / ENV-CONTROLS-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a022` | ENV10项缺陷及匿名volume复审 | 只读infra/a019；仅a022证据；无真实资源修改 | CLI终态0，request_changes；相对冻结入口绕过、换行log误删、health吞psql错误3项；旧4构建仅对应封存快照 |
| 00C / HARNESS | `eef831aa-dba4-4af2-a78b-541bc9e0a023` | SP00-W05 | crates/xtask、ci/workflows/evidenceSchema、根workspace/lock唯一writer、arch成员清单最小同步、新a023证据 | CLI终态0，in_review；manifest/commands存在；audit136依赖0漏洞、deny0errors有syn重复warning；W04/infra真实整合与独立审阅仍待补 |

HARNESS启动后根workspace/lock归其单writer，新增xtask成员同步架构成员门禁（不改安全逻辑）。CORE仅core/contracts，DATAGEN仅bench独立workspace，ENV复审只读。HARNESS不整合仍在写的datagen，也不将未实现suite或缺环境报告为通过；最终串行integration pass对W04与当前root/核心一起签收。

| 波次 / 槽 | CLI session | 任务 | 写入范围 | 状态/证据 |
|---|---|---|---|---|
| 00B-R4 / CORE-BOUNDARIES-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a024` | 四个CORE边界修复独立审阅 | 只读core/contracts；仅新a024证据与独立临时探针 | CLI终态0，request_changes；API/Arrow切片approve，Unicode/version诊断泄漏及JSON零归一化未过；core55tests/Clippy复验0 |
| 00B-F4 / ENV-RELATIVE-CLEANUP-FIX | `eef831aa-dba4-4af2-a78b-541bc9e0a025` | a022三项拒绝/清理/health修复 | 仅infra/a025证据/自己UUID资源；根只读 | CLI终态0，in_review；REPORT/dispositions/34文件hash与43红失败→0绿失败、一次真实fixture driver0；新Linux集成4构建待全写者冻结 |
| 00C-R / DATAGEN-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a026` | SP00-W04独立审阅 | 只读bench/datagen/core；仅a026证据/独立临时探针 | CLI终态0，request_changes；DG01–08五假开关/四缺主键/DDL漂移/路径CLI及角色攻击fixture；独立xs/s重复字节一致，完整任务未签收 |
| 00B-F5 / CORE-UNICODE-ZERO-FIX | `eef831aa-dba4-4af2-a78b-541bc9e0a027` | CB1–8修复 | 仅core/schema/a027证据，根与gold不改 | CLI终态0，in_review；DISPOSITION/61红失败→141绿PASS/60coretests与Clippy0；待独立复验 |
| 00C-F / DATAGEN-CORRECTIONS | `eef831aa-dba4-4af2-a78b-541bc9e0a028` | DG01–08修复 | 仅bench/datagen与索引/a028证据 | CLI终态0，in_review；REPORT/32文件hash、20tests/Clippy0、32声明smell逐项及xs/s重复证据存在；待复审 |
| 00C-R / HARNESS-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a029` | W05/17成员独立审阅 | 只读xtask/CI/root/arch；仅a029证据/临时探针 | CLI终态0，request_changes；HN01–14能力激活/基线信任/永远pendingwrapper/Secret/deny/evidence及年轻lock等；11xtask/43archtests通过不替代门禁 |
| 00B-R5 / ENV-RELATIVE-CLEANUP-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a030` | a025三修复独立审阅 | 只读infra/a025；仅a030证据与fake资源 | CLI终态0，request_changes；原三路径hold，复制/hardlink及OO_COMMON_SH信任绕过、builder路径误拒合法stage三新缺陷 |
| 00C-F / HARNESS-CORRECTIONS | `eef831aa-dba4-4af2-a78b-541bc9e0a031` | HN01–14修复 | xtask/CI/evidence/supply记录/rootCargo及lock唯一writer；其他源只读 | CLI终态0，in_review；REPORT/dispositions/29源hash存在，19xtask/43archtests0，根119registry年龄全过且audit/deny0；真实wrapper与保护策略待集成 |
| 00B-R6 / CORE-UNICODE-ZERO-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a032` | CB01–08复审 | 只读core/schema；仅a032证据/独立探针 | CLI终态0，request_changes；JSONzero/API/Arrow批准切片；encoded-colon诊断/BatchVersion回显/裸IPv6三剩余；60coretests及Clippy0 |
| 00B-F6 / CORE-AUTHORITY-BATCH-FIX | `eef831aa-dba4-4af2-a78b-541bc9e0a033` | a032三诊断修复 | 仅core/schema/a033证据；根只读 | CLI终态0，in_review；DISPOSITION/43红失败→95绿PASS、62coretests/Clippy0；schema未改变，待复审 |
| 00B-F6 / ENV-COPY-BOOTSTRAP-FIX | `eef831aa-dba4-4af2-a78b-541bc9e0a034` | A0301–3修复 | 仅infra/a034证据/自己UUID资源 | CLI终态0，in_review；REPORT/dispositions/76红失败→86绿PASS、freshfixture0；common指纹faee8f...，待独立复审与当前4构建 |
| 00C-R2 / DATAGEN-CORRECTIONS-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a035` | DG01–08复审 | 只读bench/datagen；仅a035证据/临时探针 | CLI终态0，approve修复切片；192checks/0failure、锁定测试Clippy release0，32源文件tree匹配a028；完整AT-EVAL01/SP00/SP12及最新核心平台集成未签收 |
| 00B-R7 / CORE-AUTHORITY-BATCH-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a036` | a033三修复复审 | 只读core/schema；仅a036证据/临时探针 | CLI终态0，approve纯模型修复切片；273探针零失败/62tests与Clippy0/43文件hash匹配a033，F0入口可消费；完整服务/AT/最新平台未签收 |
| 00C-R3 / HARNESS-CORRECTIONS-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a037` | HN01–14复审 | 只读xtask/CI/root；仅a037证据/临时探针 | CLI终态0，request_changes；7剩余missing changed/bootstrap自批/HEADbase/Cargo cwd/workflowlog绑定/wrapper/deny/bench年轻lock与短URL密码；其余证据预算等持平批准切片 |
| 00B-R8 / ENV-COPY-BOOTSTRAP-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a038` | A0301–3复审 | 只读infra；仅a038证据/fake资源 | CLI终态0，approve修复切片；34hash匹配、self-check0、13common指纹匹配；首失败是旧临时文件统计误判已单独复验；完整四构建仍待执行 |
| 00D / CURRENT-LINUX-MATRIX | `eef831aa-dba4-4af2-a78b-541bc9e0a039` | 当前17成员源码Linux/MSRV四构建 | 仅a039证据/当前sealed快照/自己的UUID builder资源；源只读 | CLI终态0，验收失败；四fetch0但build/test/Clippy及各层exit101，snapshot漏contracts/evidence include_str，无测试执行；资源cleanup完成、旧11volumes保留 |
| 00D-F / HARNESS-TRUST-INTEGRATION-FIX | `eef831aa-dba4-4af2-a78b-541bc9e0a040` | a037七剩余修复 | xtask/CI/evidence/rootCargo锁唯一writer/a040证据；不写bench/infra | CLI终态0，in_review；七项红绿作者证据、多lock audit/deny0及自包含evidence自检；仓库secrets实际1，真实wrapper/四平台/外部CI未签收 |
| 00D-F / SNAPSHOT-BUILD-INPUTS-FIX | `eef831aa-dba4-4af2-a78b-541bc9e0a041` | a039snapshot输入完整性 | 仅infra/a041证据；根只读，不跑移动源四构建 | CLI终态0，in_review；153文件输入快照及192 self-check作者证据；仍有xtask历史证据self-test耦合，四构建未跑 |
| 00D-F / DATAGEN-LOCK-ALIGNMENT | `eef831aa-dba4-4af2-a78b-541bc9e0a042` | bench五年轻依赖修复 | 仅benchlock/依赖README/a042证据；核心及root只读 | CLI终态0，in_review；仅lock/README改动，115 registry年龄全过作者证据；test/Clippy/release0与xs/s字节匹配，audit/deny未跑 |
| 00D-R / SNAPSHOT-INPUTS-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a043` | a041独立复审 | infra只读；仅a043证据/自有临时探针 | resume终态0，approve快照修复切片；独立192 self-check与缺失/突变/hash/symlink拒绝和schema编译0；完整四构建未签收 |
| 00D-R / DATAGEN-LOCK-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a044` | a042独立复审 | bench/core只读；仅a044证据/自有临时探针 | CLI终态0，approve lock修复切片；115 registry零问题、测试Clippy release实际exit0及xs/s全部文件匹配；完整W04/平台/audit未签收 |
| 00D-R / HARNESS-TRUST-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a045` | a040七剩余及历史耦合独立复审 | 所有源只读；仅a045证据/自有临时探针 | CLI终态0 request_changes；12修复切片持平，HN11测试runner无log/echo伪证据、HN13缺yanked放行；350源hash稳定；门禁未签收 |
| 00D-F / HARNESS-LOG-METADATA-FIX | `eef831aa-dba4-4af2-a78b-541bc9e0a046` | HN11/HN13窄修复 | xtask/CI/evidence contract及root唯一writer；仅a046证据 | CLI终态0 in_review；25xtasktests/Clippy作者0、23红绿探针与capture契约；execute-local未实跑，secrets仍1；根再次冻结 |
| 01A / DB-PERMISSIONS-SPIKE | `eef831aa-dba4-4af2-a78b-541bc9e0a047` | SP01 P6/P13真实PG权限原型 | testing/spikes/p06-p13独立Rustworkspace/指定新ADR/a047证据/自己UUID资源 | CLI终态0 in_review；真实PG17主从73truth/11tests作者green、TLS/pool/40001证据；SETclaims可绕过/domaincast忽略/maskraw暴露如实记录，ADR Proposed；完整P未签 |
| 00D-R / HARNESS-CAPTURE-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a048` | a046独立复审/本地真实capture路径 | 源只读；仅a048证据/自有临时仓库 | resume终态0 request_changes；HN13严格metadata批准，HN11公共binding自制capture仍required_satisfied、executor无deadline；真实本地spawn已观测，未签收 |
| 00E / LINUX-MSRV-MATRIX | `eef831aa-dba4-4af2-a78b-541bc9e0a049` | 新快照四平台构建 | 源只读；仅a049证据/自有UUID最多2builder | resume终态0，REPORT完整；154文件冻结快照，四fetch/build/Clippy0但test101（122pass/1fail），缺benchlock；outer101/driver0，cleanup0，未签收 |
| 01A-R / DB-PERMISSIONS-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a050` | a047真实独立复审 | spike全部只读；仅a050证据/独立UUID2PG资源 | CLI终态0 approve仅host观察；独立73truth但softgold、inheritTRUEcustomer电话越权、硬编码UUID、Clippy101、license/TLS/BYPASS探针缺口；全部父项未签，须修复 |
| 00F / HARNESS-EXECUTION-TRUST | `eef831aa-dba4-4af2-a78b-541bc9e0a051` | HN11真正执行信任/deadline | xtask/CI/contracts-evidence/root单writer/a051证据 | CLI终态0 in_review；34xtasktests/Clippy作者0，导入capturepending、私有执行值/timeout与真实9gate路径；secret1/外部CI未签 |
| 00F / SNAPSHOT-BENCH-LOCK | `eef831aa-dba4-4af2-a78b-541bc9e0a052` | 四平台缺benchlock修复 | 仅infra/a052证据；不跑移动rootmatrix | CLI终态0 in_review；REPORT存在、200selfcheck作者0、缺失/突变benchlock拒绝；155输入但root两文件漂移，不签集成；待独立review |
| 01B / DB-PERMISSIONS-CORRECTIONS | `eef831aa-dba4-4af2-a78b-541bc9e0a053` | a050权限/重跑/真值/许可证修复 | 仅spike/指定ADR/a053证据/独立UUID2PG | CLI终态0 in_review；REPORT存在，真实修复实验/许可证及兼容scanner作者证据；仅provenance最终hash变动；待独立复审 |
| 00F-R / SNAPSHOT-BENCH-LOCK-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a054` | a052独立复审 | infra只读；仅a054证据/独立临时探针 | CLI终态0 approve快照修复切片；独立200selfcheck、双copy/missing/mutate/symlink拒绝；36infra稳定/旧a049不变；完整四平台未签 |
| 00F-R2 / HARNESS-EXECUTION-TRUST-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a055` | a051独立复审 | 全源只读；仅a055证据/自有临时仓库 | CLI终态0 request_changes；导入capture/private类型修复hold，setsid继承pipe/PATHkillno-op两hang需outerSIGKILL；9gate真实8pass/secret1，24focus稳定 |
| 01B-R / DB-PERMISSIONS-CORRECTIONS-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a056` | a053真实独立复审 | spike只读；仅a056证据/自有UUID2PG | CLI终态0 approve修复切片；独立107truth/14tests/Clippy0、错数据/授权probe exit1/8fail；许可证18gap及MSRV/amd64/P14/完整P未签 |
| 00G / LINUX-MSRV-MATRIX | `eef831aa-dba4-4af2-a78b-541bc9e0a057` | 当前冻结快照四平台 | 源只读；仅a057证据/自有UUID2builder | CLI终态0 REPORT完整；四fetch/build/Clippy0但test101各133pass/6fail，CLIgitcommit/fixturechild失败；benchlock缺口已过，155快照a207de...稳定；cleanup完成 |
| 00H / HARNESS-DEADLINE-LINUX-FIX | `eef831aa-dba4-4af2-a78b-541bc9e0a058` | a055两hang及a057六CLI平台失败 | xtask/root唯一writer/minimalCI/harnessdocs；仅a058证据/自有UUID单builder | finalize终态0；代码f61de8d、交接9c2ed1d；REPORT/HANDOFF齐备，host/Linux1.98/Linux1.94各41局部测试及Clippy作者0；a062固定SHA复审，完整SP00未签 |
| 00I / DATAGEN-LINUX-MSRV | `eef831aa-dba4-4af2-a78b-541bc9e0a059` | standalone bench双架构/MSRV实跑 | bench/core只读；仅a059证据/自有UUID单builder | finalize CLI终态0，报告commit275d886；四recipe测试/Clippy/生成作者0，bytecompare0/cleanup0；outer1原因已读脚本1104–1106：0 or 1误记，旧143/limits1不重写；未签完整W04 |
| V01-PG-01 / PRODUCT-SOURCE-CATALOG | `eef831aa-dba4-4af2-a78b-541bc9e0a060` | 真实PG只读体检与catalog生产组件 | 现有product-pg worktree，仅connectors/lock/新source契约测试；本地提交 | CLI终态0，feature b2d3cbc，baseline4213134；13unit+live PG17/Linux1.94测试Clippy作者0、REPORT已提交；in_review，不签完整SP03 |
| V01-PG-01-R / FIXED-COMMIT-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a061` | b2d3cbc固定SHA独立复审 | 现有productworktree只读；仅主目录a061证据/自有临时archive/UUID | CLI终态0 request_changes；完整review/findings已读：三HIGH为全局写能力漏判/PGOPTIONS角色替换/CA根混用；Linux/MSRV及原live测试0，audit0/deny4；owned资源已清，a066返工 |
| 00H-R / FIXED-COMMIT-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a062` | f61de8d固定SHA基础修复 | 只读git archive；仅a062证据/自有UUID资源 | resume handle37699终态0 request_changes；Linux1.94 xtask41测试/Clippy0，但9个early-setsid后代存活仍required_satisfied，15/15复现；a070修复，原probe编译101保留 |
| V01-OOS-01A / STRUCTURE-DRAFT | `eef831aa-dba4-4af2-a78b-541bc9e0a063` | 冻结core模型/PG catalog版本契约 | 主目录oo-builder/新draft契约fixture；交付后root lock冻结 | handle70712终态0 commit4c989bc；REPORT/HANDOFF已读、Linux1.94原始18lib+2CLI测试/Clippy0、shop黄金hash协调者重算匹配；a071复审，不签真实PG闭环或完整SP07 |
| SP02-W01-a / CONTROL-JOBS-SCHEMA | `eef831aa-dba4-4af2-a78b-541bc9e0a064` | 导入闭环所需耐久控制存储，docs17队列契约 | oo-jobs迁移/契约/对应fixture；不改manifest/root-lock，a063仍唯一writer | CLI终态0 commit70a4e72；REPORT已读，真实PG新迁移/重放/40 SQLSTATE负例/迁移锁/失败回滚作者0，Linux/MSRV contract1test0；migration两hash协调者重算匹配，a068复审，不签完整W01 |
| P5-a / PG17-PARSER-RISK | `eef831aa-dba4-4af2-a78b-541bc9e0a065` | 受控查询的parser/PG17配套与可杀隔离风险；a056 SET utility反例 | 新独立Rust spike/lock、Proposed ADR/a065证据；不写主workspace依赖 | handle52338终态0、交付efb61d6；REPORT已读，原型PG17.7/真实17.10差分、Linux1.94 locked tests/Clippy作者0；R4/R9缺口保留，a076复审。CLI末尾actor异常warning保留，不当安全验收 |
| V01-PG-01-F / SOURCE-SECURITY-FIX | `eef831aa-dba4-4af2-a78b-541bc9e0a066` | a061三HIGH固定b2复现，完整review已终态 | 沿用现有product-pg，交付后源冻结；主目录源未整合 | handle37636终态0 correction4ecdbf7/stamp181df9b；REPORT/patch已读，Linux1.94原始14lib+1live/Clippy0，lockhash重算匹配，webpki已移除但旧ISC策略deny4；a074固定复审，未签修正 |
| 00J / ISC-POLICY-CONSISTENCY | `eef831aa-dba4-4af2-a78b-541bc9e0a067` | a061固定图ISC3项许可门禁失败、spec宽松许可规则 | 仅ci/deny的明确ISC条目、新license契约/fixture/a067证据；不改root-lock或source | handle38831终态0 commitb14c782；REPORT已读、policy hash协调者重算匹配，作者Linux1.94 core check0，b2许可错误4→1但deny仍4；a069复审，不签完整供应链 |
| SP02-W01-a-R / FIXED-SCHEMA-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a068` | 固定70a4e72 schema交付/原始PG证据 | 源只读git archive；仅a068证据/自有UUID资源 | handle57161终态0 approve schema切片；独立真实LOGIN权限/并发/回滚等117case0fail，Linux1.94 test/clippy0；F1–F5残余归后续任务，不签完整W01 |
| 00J-R / FIXED-ISC-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a069` | 固定b14c782许可切片 | 源只读archive；仅a069证据/自有UUID资源 | handle41829终态0 request_changes仅attribution：r-efi AUTHORS含MIT正文，旧packet误称无正文；ISC规则/负例独立hold，a077修正不阻塞产品；不签完整W06 |
| 00K / HARNESS-ESCAPE-CONTAINMENT | `eef831aa-dba4-4af2-a78b-541bc9e0a070` | a062终态early-setsid假成功复现 | 仅xtask src/tests/minimal harness契约及a070证据；不写rootmanifest/lock | handle31198终态0，交付5ef3d6d/stamp0fcc2f9；C运行时编译不符合Rust要求，两旧ISC shape tests失败，a082基础纠正，未验收 |
| V01-OOS-01A-R / FIXED-DRAFT-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a071` | 固定4c989bc纯结构草案 | 源只读archive；仅a071证据/自有UUID资源 | handle11184终态0 request_changes；五findings完整证据已读；a081先修结构closure，NFC依赖修正待a073 root交接，不签SP07 |
| SP04-W05-a / TARGET-FENCE-RECEIPTS | `eef831aa-dba4-4af2-a78b-541bc9e0a072` | docs17目标同连接锁与receipt、已冻结Batch/position契约 | 仅oo-pipeline新迁移/contract/tests/自身fixture证据；不改root依赖 | 原两次终态不完整后同UUID续接handle79046终态0，交付8370498；REPORT/原始局部0/fromemptyfixture已读，a085固定复审，未签完整Loader |
| SP09-W01-a / SDK-JSON-QUERY | `eef831aa-dba4-4af2-a78b-541bc9e0a073` | frozen core HTTP API/docs08查询契约 | 仅oo-sdk/新SDK contract/fixture/a073证据；main root-lock唯一writer | handle23139终态0，交付8efeb23/stamp0b544cb；REPORT及原始Linux0已读；a084固定复审，root交接a083，不签真实PG闭环 |
| V01-PG-01-F-R / FIXED-SECURITY-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a074` | 固定4ecdbf7安全纠正与原a061三HIGH | 现有PG树只读archive；仅main a074证据/自有UUID资源 | handle29369终态0 approve仅纠正切片；完整review/raw已读，当前图root0、冲突图119，合并图约束/许可正文待整合 |
| SP02-W01-c / EVENT-PROJECT-BINDING | `eef831aa-dba4-4af2-a78b-541bc9e0a075` | a068-F2已复现事件跨项目，v1 schema已签子项 | 仅jobs新expand/contract/tests与自有fixture证据，不改root/v1 | handle46905终态0，交付9224d31；REPORT/HANDOFF与原始SQLSTATE/局部Linux exits已读，a079固定复审，不签队列驱动 |
| P5-a-R / FIXED-PARSER-REVIEW | `eef831aa-dba4-4af2-a78b-541bc9e0a076` | 固定efb61d6语法/IPC/star原型 | 只读standalone archive；仅a076证据/自有UUID资源 | handle88198终态0 request_changes；矛盾allow响应和IPC无限等待已复现，完整review/raw已读；a080仅独立spike修复，完整P5未签 |
| 00J-F / ISC-ATTRIBUTION | `eef831aa-dba4-4af2-a78b-541bc9e0a077` | a069 r-efi MIT正文记录错误 | 仅ISC当前notice/policy/provenance及新erratum/a077证据；ci/old证据/root不写 | handle85459终态0，交付fb2d1d1；REPORT/HANDOFF已读，Linux1.94 core check作者0，外层inspect exit1保留；a078固定归属复审，未签完整W06 |

## 5. 用户优先级调整后的产品功能台账

| 功能任务 | 输入与前置 | 输出/接口 | 验收与下一步 | 当前状态 |
|---|---|---|---|---|
| V01-PG-01 | 已批准core模型、docs PG只读安全边界；现有隔离任务不等基础门禁 | PG AccessReport+CatalogSnapshot/F1版本契约，真实SQLx只读driver | Linux/MSRV局部+真实PG源审计零写/TLS/权限，交付commit后固定SHA复审 | b2初审request_changes，a066交付修正4ecdbf7，a074固定复审；旧registration_allowed不可放行，未勾源验收 |
| V01-PG-02 | V01-PG-01固定接口、目标事务/质量/fence设计 | 有界PG读取→目标LOGGED未发布导入与持久catalog/lineage | 真实source/target比对、失败重启/取消、不提前授权 | waiting接口；按SP02/SP03/SP04拆可验收子项 |
| V01-OOS-01 | catalog/安全profile证据、OOS纯模型 | 确定性可审核草案与证据/PII标记，不自动发布 | 真schema到draft、恶意comment/歧义/权限无扩张；固定SHA审阅 | a063交付4c989bc、a071复审中；真实导入集成与完整profile待后续，未勾完成 |
| V01-QUERY-01 | 批准policy/P6/P13切片、P5 parser/P14驱动协议 | 审批后角色列权限/RLS、HTTP Gateway查询/取消/审计 | 实际隐藏行列拒绝、claims不伪造、parserfailclosed、取消已停 | queued |
| V01-CLI-01 | SDK/查询+受审draft/apply接口、runtime模型出站安全 | CLI自然语言交互→受控查询，真实PG闭环 | 端到端同主体/审核/权限/输出，未配模型明确错误不假答 | CLI queued；其独立HTTP query SDK前置a073已启动，不以协议fixture替代真实对话验收 |
| FOUNDATION-LANE | a058两hang/六CLI失败、a059bench平台 | 基础问题单线收敛，按局部失败修复 | 不阻塞隔离产品公共功能；发行条件单独pending | active |

当前生效边界：a069已终态，ISC记录纠正a077仅notice/policy/provenance/新erratum；基础harness a070仅xtask源/测试和必要契约，两者不共享root写入。a063已交付4c989bc，a071只读复审；main root-lock由a073独占，源整合等待其终态再串行。a064的schema切片已获a068批准，a075仅jobs新expand/测试；a065已交付efb61d6，a076只读standalone原型；a066已终态，现有PG源码冻结4ecdbf7，a074只读复审；a072仅pipeline新迁移/契约/测试。所有固定提交复审不读取移动源作签收依据；最多2个Rust构建者计入host cargo构建。

a062终态证据：协调者已读完整review/findings及seal-escape原始JSON。九个睡眠后代在父进程立即退出后仍存活，执行exit0/required_satisfied=true且无cleanup/deadline门禁；15/15重复复现，held inode在返回后继续增长。仅记录后代的轮询不能证明所有后代已停。a070须修复或在归属无法验证时明确失败，不能将16MiB内存前缀当磁盘配额；既有41测试通过、PATH固定kill/隔离Git/private ExecutedCommand修复保持，未复现的kill故障不伪称已证实。基础返工不计新产品功能，完整SP00仍未签。

a066所发现的SQLx API限制已由协调者核查锁定0.9源码，最小纠正方向冻结于execution/decisions/sqlx-0.9-explicit-source-trust.md（commit1589c18）。方向保留SQLx，必要时仅对两上游crate作有来源/许可/补丁hash的无env构造器与显式CA独占补丁；当前a066仍运行，不能同时启动另一writer。其真实终态/交接后再以明确resume扩展允许文件范围，实施与独立复审未完成前不签修复。

a066最新实际状态：终态0，代码4ecdbf7、stamp181df9b。两个vendor加入无env构造器及ring-no-roots feature；报告称247依赖/0新增registry、移除webpki/CDLA，作者audit0但旧ISC策略deny4。协调者已读REPORT/PATCH与raw live3，通过14lib+1live/Clippy0、lockhash重算匹配；未接受安全结论。a074须复核精确upstream差异、加性feature根混用风险、补丁SBOM身份/来源与完整license，后续有限snapshot包含vendor必要输入，不复制整个registry/cache。

导入集成待验边界：pgident/v1的q_加完整NFC字节hex映射可能超过PG63字节标识符上限；例如31字节大写名映射为64字节。两个仅末字节低nibble不同的名称可在目标PG隐式截断后碰撞。此为固定b2代码静态发现的目标集成风险，尚未做真实PG复现，需在SP03-W05/SP04目标命名持久映射阶段验证；logical catalog id不能未经长度/碰撞核验直接充当目标DDL标识符。不得在当前移动草案上直接修改契约或把静态推导当完整验收。

完整Spec/Plan/87ToDo/16Phase保持，功能台账是交付顺序，不是缩小目标。完成勾选只按功能真实证据和固定SHA独立验收；parentPhase/AT不得因子项commit提前勾。开始记录功能交付、验收工作项与返工率；既有基础修复切片不冒充完整产品功能。

a059与a058并行：a058唯一rootwriter/最多1debugbuilder，a059仅读冻结bench/core/根manifest并构建其自己封存的独立生成器输入/最多1builder。生成器平台验证不读取移动xtask作为产品依赖，不签根快照。总并发builder最多2，资源UUID隔离。

历史边界快照（已被上方当前边界替代）：a058已提交，a062只读固定archive复审；a061只读PG固定commit；a063唯一builder/root-lock writer；a059已终态，不再占用builder。总并发builder最多2，资源UUID隔离，完整双架构矩阵放在集成节点。a056权限切片已批准，但18licensegap/未执行MSRV和amd64/P14/完整P仍pending。

当前产品速度：已交付可验收产品功能1项（PG源体检/catalog，固定b2d3cbc，但未验收）；本批产品功能独立验收0项；已完成首轮复审的产品交付1项，其中需要返工1项，返工率1/1，样本仍小。修复commit不重复计为新功能，不用基础修复次数或代码行数替代。完整87工作项及16Phase父项未提前完成。

新增交付：控制jobs schema子任务70a4e72（a064），交付任务累计2项＝PG入口功能1项＋控制schema组件1项；独立验收仍0项，schema尚在a068复审，不重复计为完整jobs服务。source首次复审需返工1/1；schema未出独立结论，不纳入已复审样本。完整87父工作项/16Phase仍未完成。

最新产品速度：累计交付3项＝PG入口/catalog＋控制schema＋纯结构草案。独立验收子项1项＝SP02-W01-a；完整父工作项0项。已完成首轮产品复审2项，其中源需返工1项，返工率1/2（小样本）；a071未出结论不计入分母。a066修正不重复计新功能，a075事件绑定为新工作但未交付。ISC/harness是基础线，不冒充产品功能。真实源读取/目标导入、安全审核到查询CLI仍未闭环，a074安全复审及a073 root-lock交接后按源范围串行整合。

a061复审中间证据：真实probe已发现所选表外写权限仍registration_allowed；audit实际exit0，deny实际exit4（ISC/CDLA-Permissive-2.0未列允许策略）。结果来自a061自有冻结archive探针，不是作者声明；最终findings/最小复现尚未交付，暂不签源注册，后续收敛范围不能仅检查selected tables。此问题不阻塞纯builder或控制迁移，许可修正需逐许可证正文/义务与来源，不做通配豁免。

以上中间记录已由a061最终request_changes替代：review/findings提交4a65a37，三HIGH均已派a066，旧b2生产准入未签。基础工作线仍需单独收敛ISC许可证策略与实际正文义务；CDLA依a066最终TLS特性图是否仍存在确定，不以许可调整替代private-CA-only修复。已有a056的18项正文缺口保持独立pending。

执行命令纠正：a063首attempt101与a064首attempt1均含cargo fetch错误-p参数；后续任务引用execution/prompts/linux-msrv-focused-checks.md固定合法fetch/test/Clippy命令，保存原失败exit，局部重跑不重复全矩阵。a063第二attempt已过；a064仍执行中，不能沿用其他任务的成功声明。草案固定提交复审还必须比对pgident/v1 Unicode/NFC版本与PG契约，手工生成Unicode数据也需来源、许可证及独立一致性证据。

a059证据收尾handle82531已终态0，提交275d886仅REPORT/manifest/HANDOFF，未重跑矩阵或修改原失败exit；a064迁移任务与a063按源码分区并行，所有CLI写同一Git index时必须明确暂存自有文件，提交前核对index无外来路径，否则等待协调者串行提交。

最新状态：所有writer终态，root/infra/bench/core/spike冻结；a055/a056只读复审与a057当前四构建并行，资源UUID隔离。P6/P13修复仍in_review；完整87工作项/16Phase与父AT未提前勾选。

最新writer边界：a051唯一root/harness，a052仅infra有限输入清单，a053仅独立spike/ADR；三者可并行但全rootmatrix等待a051/a052终态及复审冻结。a050 host观察批准不免除实际发现的修复项，也不勾完整P/AT；a048 HN11真实绕过阻断门禁，公开hash只是integrity不能当authority。

最新冻结：a046已终态，根/core/infra/bench源码冻结；a048只读复审与a049有限快照四构建并行，a047仅snapshot排除的spike路径写入。a049资源与a047各自UUID分离，负载过高串行builder。Linux矩阵证明实际快照编译测试，不替代harness信任复审或完整AT。

最新工程界限：a046唯一根/harness writer，a047仅隔离权限spike和新ADR，两者无共享源写入。P6/P13依SP00薄原型和真实PG节点，可先小规模验证DEC03/DEC13以解除SP06风险；最终SP06仍依SP02/SP03、P5/P7/P14及独立审阅。root四Linux构建等待a046终态和冻结，不能以移动源码宣称当前验收。全部87工作项/16Phase父项仍未签收。

新一波写入界限：a040仅根/工程门禁，a041仅测试基础snapshot清单，a042仅bench独立lock。三者不重叠；a040最终多lock扫描等待a042终态。a039四失败是真实失败证据，不能标为Linux/MSRV通过。修复和复审后再次冻结完整输入实跑四构建，bench平台另验。

最新交接：a041/a042均已终态，infra/bench冻结，a043/a044只读独立复审；a040仍唯一根writer。a040须消费a042新benchlock SHA 8fe0a83684a72239e116339349a44dada871ba3523f5ca718871c5690faa6df5进行真实多lock扫描。a041发现xtask evidence self-test依赖a011历史manifest；须改为自包含合成fixture或最小公开契约fixture，不能将历史evidence整树加入构建快照来伪造通过。该跨模块补修归根writer串行处理，最终Linux矩阵等待修复冻结。

a039开跑时产品源码冻结，a037只读review。CORE a036与DATAGEN a035、ENV a038各修复切片获独立批准；根17成员快照可验证当前Linux/MSRV构建，但bench独立workspace平台与HARNESS可信门禁仍另行验收。

所有产品writer已终态，根Cargo/lock、core、bench、infra冻结；a036/a037/a038只读并行复审。DATAGEN a035仅批准DG修复切片。实际统一快照/四构建与W05真实wrapper集成按独立结论推进，外部保护策略和真实owner不虚构通过。

当前并行writer：a031根/xtask/CI，a033仅core/schema诊断，a034仅infra；a035只读数据生成器。写入区互不重叠，根锁仅a031。实际集成与四构建必须等待各自复审和当前源码冻结，不能用切片批准替代完整AT。

a031启用后根Cargo/lock再次只归HARNESS修复writer；a032只核对稳定core/schema与实际根快照，不把移动锁当冻结事实。DATAGEN a028仍写bench，ENV a030只读infra。实际W04/infra整合分支必须实现，但执行需等待独立review签收和协调者冻结输入；永久pending骨架不算W05完成。

00C下一波：a023终态后根Cargo/lock冻结，无其他writer；a027 CORE和a028 DATAGEN各自独立目录并行，a025 ENV仍live。a029 HARNESS只读审阅冻结工程文件，不用移动CORE/infra签收完整集成。依赖顺序为三修复→各自独立复审→W04/infra串行接入HARNESS→冻结完整源码→实际4组Linux/MSRV构建及阶段门禁。禁止把pending wrapper当作已完成W05。

当前顺序：a020已终态→a024独立审阅；a021/a022/a023实际handle仍live，可与只读CORE复审并行。根文件仍由a023单writer。CORE复审不得写核心或以移动根工作区声称完整快照通过。上一次目标回合完成Plan补充并取得文档验证证据，归类为progress；这不改变任何产品门禁状态。

00C并行：CORE-BOUNDARIES仅共享模型4处修复，DATAGEN只消费a018已批准的规范值解析/日期/时区/版本切片并在bench/datagen独立workspace开发；root成员与lock保持不变。ENV恢复只写infra/自身资源，可构建实际稳定快照。HARNESS后续串行整合workspace/lock与架构成员清单，并等待W04fixture到位；所有完整Phase门禁不提前签收。

CORE-HARDENING期间重新开放oo-core/root写入。ENV只可构建其实际冻结快照并记录hash，不能将并行复制/旧快照宣称当前接口验收；后续在CORE修复终态后补当前源双架构。DATAGEN/HARNESS仍等待协议复审冻结。

CORE接收当前可用的macOS骨架和契约开发前置，不代表SP00-W01完整AT已签收。Linux双架构由ENV冻结源码快照实际构建补验。CORE是00B期间root依赖与lock唯一writer；只读review不得format/fix；ENV仅写独立基础设施，不与CORE冲突。

协调者已修正Plan/README过时的“无workspace”说明，并重新通过Spec/Plan检查。该文档维护不改变任何产品要求；先前证据中的Plan hash保留其原运行快照，后续证据记录当前实际hash，不回写旧证据。

初始日志位于`/tmp/open-ontology-grok-00a-foundation.log`和`/tmp/open-ontology-grok-00a-governance.log`。每次结果先核对实际进程终态/文件，再由新Grok只读审阅；缺陷修复必须指定失败证明，不用`--continue`误接另一并行会话，使用明确`--resume <UUID>`并再次指定模型/effort。

## 当前交接补充：a077 → a078

a077实际终态0，固定提交fb2d1d19c21b510dfd6231c3b67d0efbf4285491（tree79f19b818d3e14ad08ad5208ce2bd0a17b4c297e）。协调者已读完整REPORT/HANDOFF，当前已提交process evidence仅173字节、凭据参数标记计数0，git fsck connectivity exit0；此有限检查不证明所有历史副本清除。执行者报告最初进程命令行记录含Cursor API密钥及session token、未push，并已删除相关loose objects。已告知用户轮换Cursor密钥；独立a078须核验当前提交和可见对象的残余，但不得打印秘密、读取凭据存储或修改Git历史。秘密门禁与完整供应链仍未验收。

a078仅只读固定archive、自己的证据与UUID资源；先Linux/MSRV局部冒烟，再归属正文/erratum/旧记录不改/现有策略不弱化复核。a073仍唯一main root-lock writer，a070仅xtask、a072仅pipeline新迁移、a075仅jobs新expand。基础线修正不改变产品交付/验收计数，不阻塞无关公共功能。ToDo归属修正仍待独立结论，完整W06/87父工作项/16Phase不提前勾选。

## 当前交接补充：a075 → a079

a075实际终态0，固定交付9224d31e1c1d4a60d44b6984b8ad496a09e706a1（tree9e4bd758216287ee78c162349950b988da25978b）。协调者已读完整REPORT/HANDOFF和0002 SQL，重算0000/0001/0002校验和；0002为4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee。原始Linux1.94 baseline与implementation tests/Clippy退出0；PG跨项目/并发跨项目23503、坏历史OO004、runtime重放42501已核查，坏历史/合法升级/强制rollback的前后数据snapshot逐字节相等。作者历史四轮fixture失败与最初漏contracts的101保持，不当最终成功。

a079仅只读该固定archive、独立证据和UUID资源，先Linux/MSRV局部冒烟再真实PG新装/升级/并发/身份/负例，复审前SP02-W01-c仍不勾选。旧已签SP02-W01-a不撤销；完整W01/SP02/驱动/CAS/备份/100worker/目标fence与纵向闭环仍待验。a073仍唯一root-lock writer，a070/a072仍各自源码writer；其余review只读。

最新产品速度：累计交付4项＝PG入口/catalog、控制schema、纯结构草案、事件项目绑定；独立验收子项仍1项（SP02-W01-a），完整父工作项0。已完成首轮产品复审仍2项、返工1/2，小样本；a071/a079未终态不计入分母。a075修复既有发现但新增独立交付任务只计一次；基础许可证/harness修正不计产品功能。

## 当前交接补充：a076 → a080

a076实际终态0，对efb61d62803073b3f98f5fe1885877824f504f80提出request_changes。协调者已读完整review/findings、原始IPC矩阵、PG前后行数及cleanup；ipc-driver.log重算sha256为e54c3facc54c6c01e1bea95454103cba303cfa28e946773b558673a9ac8bb479。false syntax_allow配ALLOW_SYNTAX被转为true且保留DROP SQL（未发PG）；bad magic/junk/oversize及成功后pipe持有在外部8秒timeout后仍不返回，三个worker持续存活、setsid后代绕过进程组kill。已观测正常语义33oracle匹配不替代IPC安全；parser17.7/server17.10及R4/R5/R9缺口仍未签。

a080仅独立P5 spike/standalone lock/ProposedADR及自身证据，先Linux/MSRV基线后Rust实现响应一致性拒绝和每分支有界生命周期/可验证进程归属，保留真实安全隔离与许可来源缺口。不得使用移动a070 C helper或写主root；a073仍独占主依赖。a076原始1/127/apt100失败与外部124保留，复审证据提交后不回写。风险原型修正不计新产品功能，SP01/P5/完整Gateway/AT均不勾选。

## 当前交接补充：a071 → a081及NFC依赖修正

a071实际终态0，对4c989bce58407cf19d3d93dc600162d7322d92f8提出request_changes。协调者已读完整review/findings并重算phase1/nfc-run/nfc-tables三hash，与报告相符；73probe记录含拒绝bill_to后仍保留引用该列的有效FK，以及[id,id]主键。Unicode16官方vectors出现3830 bad slots，全标量191 mismatch；Unicode17新增34 combining classes不在旧表，且UCD复制缺许可notice。五项均待纠正，原MSRV18lib+2CLI通过不替代这些负例。独立golden扰动真正执行的test exit101与误过滤0test两次保留，不称作产品失败。

a081只修结构一致性findings3/4，写builder draft/pgsql/测试及契约，不写NFC/manifest/root/core；前置局部Linux/MSRV后提交并复审。NFC findings1/2/5需改用与PG connector一致的unicode-normalization0.1.25并验证官方U16/U17 vectors、全标量及增补序列，交付许可正文/来源/历史erratum；根依赖变更待a073真实终态释放唯一写权后单独派发。不能为并行绕过而维持不一致的手写规范化，也不让该依赖锁阻塞独立结构修复。

最新产品复审指标：交付4组件，独立验收子项1，完整父工作项0；完成首次产品复审3项，其中PG源与结构草案返工2项，返工率2/3（小样本）。风险P5、ISC和harness复审不计产品分母；事件绑定a079尚未终态不计分母。所有待返工项仍未勾选。

## 当前交接补充：a074/a078终态限定批准

a074 handle29369实际终态0，批准4ecdbf714766baef3490a4f954701eaf19f245a0安全纠正切片，三a061 HIGH关闭。协调者已读完整review/findings及root-store/live/SQL-audit原始证据：选定SQLx root0，额外webpki特性root119；source957语句为SELECT815/WITH58/START42/ROLLBACK42，flagged0。main整合须在a073根写权释放后串行进行，禁止覆盖旧421基础文件；合并图强制拒绝SQLx系统根/公开根及native TLS特性，vendor按modified upstream身份、补全MIT/Apache正文并重算补丁来源。当前Docker owner=a074容器/卷/network列表均空；原始audit只读mount1与后续audit0、旧deny4与ISC候选deny0保留。完整SP03-W01/13/14/读取/转换/CDC/纵向闭环仍未签。

a078 handle10405实际终态0，批准fb2d1d1归属文字切片。协调者已读review/findings及exposure-scan：8634 Git对象、6181文本blob相关凭据形态0；10 binary未扫描，历史副本/remote未查。当前提交清洁有限结论不关闭曾出现的Cursor密钥/session token暴露，用户轮换仍待确认，秘密gate未签。原smoke漏contracts101保留，补齐后62tests/Clippy0。ISC条目不撤、CDLA不添加、OR-token策略差异仍pending。已按限定切片勾选W06归属纠正及源三HIGH子项，父项和完整产品功能未提前勾选。

产品累计交付4组件；已验收产品子项2（jobs schema、源三HIGH纠正），基础ISC归属另列不充当产品功能。源安全返工不重复计新交付；首次产品复审返工仍2/3，事件a079未终态。下一mainroot任务为源范围整合并守住TLS feature组合；NFC依赖修正紧随其后，a081结构修复可独立推进。

## a072终态但不完整的续接

协调者核验handle14146 exit0，尚无任务REPORT/HANDOFF和固定交付commit，末条文本仅称fixture运行。按实际终态判定可恢复同UUID eef831aa-dba4-4af2-a78b-541bc9e0a072，保留原源码和自有PG资源继续核查现有fixture，不重启其他live任务、不重复构建无关模块、不签实现或测试成功。续接提示v01-target-fence-receipts-resume.md要求完成原验收/局部Linux1.94/清理与本地提交；仍只有pipeline新迁移/契约/测试写权，mainroot归a073。该退出不计交付功能和验收工作项。

## a070交付与Rust实现纠正

a070 handle31198终态0，交付5ef3d6d819fd72fdd5e86f79eb9e719910f7e6e0/stamp0fcc2f9。协调者已读完整REPORT/HANDOFF与containment.rs实现：第一方C源码在运行时通过可信compiler编译，不能作为用户要求Rust整体实现的最终结果。作者Linux1.94 probes称30doublefork/heldoutput/foreign保护通过，library仍32pass/2fail（旧ISC allow-shape），CLI16pass，未独立签收；CLI末尾DeadFailed warning原样保留不直接判artifact无效。

a082基础线只写xtask及新独立Rust supervisor/harnessdocs，保持主workspace和xtask unsafe forbid及root-lock不变；使用实际安全syscall封装并预构建不可变helper，不运行时编译。新standalone依赖严格核验来源/MSRV/age/license，继续真实Linux正例/逃逸归属/有界输出/失败拒绝，并最小更新已批准ISC形状检查，不放宽其他规则。a073主root依赖writer不变，产品模块照常推进。修正后固定提交独立复审；不签SP00/秘密/runsc/CI/release，也不计新产品功能。

## a073 SDK交付与主目录源整合启动

a073 handle23139实际终态0，交付8efeb230afc6734a2e50eaca3f09fd78300a9c7d（tree7d97299c1681f39db6aa5252d810a3bbb771ef41，parentd2ea635），stamp0b544cb仅REPORT/HANDOFF。协调者已读完整报告/交接及Linux1.94fetch/test/Clippy/docker0，counts5lib+18HTTP+1proxy+1TLS；root tracked diff空，写权释放。局部协议oracle不当真实Gateway/PG对话。131 registry新增/no removals；audit/deny未跑、target-all CDLA锁定依赖、nativeaws-lc许可及未归档旧Clippy失败仍pending，未签供应链。

a083现在唯一mainroot manifest/lock writer，仅源范围整合冻结4ecdbf7及其vendor，补modified-upstream身份/许可正文/强制SQLx feature组合拒绝，保持SDK已交付pins，不覆旧421基础。a084只读SDK精确archive独立HTTP/TLS/预算/重试/脱敏/供应链复审。a081只写结构closure、a080独立P5、a082基础Rusthelper、a072原UUID pipeline续接、a079事件review互不写根。NFC库依赖变更等a083终态交接，不能共享rootwriter。完整双架构矩阵仍放纵向集成节点。

产品交付累计5组件＝PG入口/catalog、控制schema、纯结构草案、事件项目绑定、HTTP JSON SDK。产品子项验收仍2（schema及源三HIGH纠正），完整父项0；首次产品复审返工2/3，SDK/事件未复审不计分母。SDK验收待a084，不勾完整SP09-W01。

a072第一次resume handle34479亦实际终态0，仍无REPORT/HANDOFF/交付commit；日志仅称复用fixture继续运行，并修正客户端错误码文本/空表delete触发器断言。第二次同UUID显式resume，保留原PG/job/evidence不重建。待真实局部测试/协议结论/清理/本地固定提交齐备才接收，产品交付计数不增加；其他live会话不重启。

## a072交付 → a085固定复审

a072第二次resume handle79046实际终态0，终于交付83704982a8d9459c51ed7c2020c51d21c91fb8cd（tree494a0f015c35e3eb75548e9d4dad71bb28d0acd7，parent78d106f）。协调者已读完整REPORT/HANDOFF及post-smoke原始fetch/test/Clippy0与从空库fixture_result=pass，重算bootstrap37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208/expand97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9。旧inplace修正及NEW.generation42703原失败保留，不能替代新空库证据。报告列两PGfixtures，资源同时性待复审查证，不默认符合max1规则。

a085只读固定archive，先Linux1.94局部冒烟，再单PGfixture独立session/temp/SDfunction/lock/generation/receipt/rollback/role/checksum复审。schema交付不代表Loader，receipt-only0/0/1、未来任意DML未fenced和控制镜像/ACK仍明确未实现；SP04-W05-a在review结论前不勾选。a083根writer不变，a081/a080/a082继续独立模块。产品累计交付6组件（新增目标schema），验收产品子项仍2，首次复审返工2/3，完整父项0。

## a079事件项目绑定限定验收

a079 handle17707实际终态0，固定9224d31e1c1d4a60d44b6984b8ad496a09e706a1/tree9e4bd758216287ee78c162349950b988da25978b获SP02-W01-c限定approve。协调者已读完整review/findings及Linux原始fetch/test/Clippy0、PG23503/OO004/40P01与rollback输出，重算合法/坏历史/强制失败六组snapshot，前后均逐字节相等且非空。首次141cases/11失败为已保留的reviewer探针错误，13followups/0失败不改写原记录；空stdout比较不能当历史不变证据。已勾选事件项目绑定子项，父W01/AT-CTL-01/02仍未签。

迁移与触发审计事件的claim并发时events→tasks锁顺序会40P01；中止不遗留约束/view/marker，任务领取提交。后续applicator必须有界超时及40P01重试/停写策略；纯lease_until heartbeat只导致55P03。stored checksum仍依赖可信installer计算与外部readiness合取，runtime仍可持advisory key，event note/generation/owner仍caller-supplied；A068-F1/F3/F4/F5继续待验。不得将schema约束当已认证worker审计。

产品累计交付仍6组件，独立验收产品子项增至3（jobs schema、源三HIGH纠正、event项目绑定）。已完成首次产品复审4项、返工2/4＝50%（小样本）；SDK及目标fence尚未终态不计分母，基础修复不计产品功能。87完整父项/16Phase仍0完成。a083继续唯一mainroot writer，a081结构closure/a080独立P5/a082基础Rusthelper、a084 SDK和a085目标schema复审各保持既定范围；下一纵向实现依源固定接口推进真实有界读取/类型转换/目标同事务写入，而非以receipt-only fixture充当导入。

## 下一纵向任务卡与当前写入风险

上一目标回合为progress：79587e4真实记录a079限定验收并勾子项。本回合一次核验a080/a081/a082/a083/a084/a085的现有执行handles均live；不因耗时或静默重启。三份prepared任务卡已按真实类型解码→同RR有界Arrow读取→目标数据/质量/receipt原子apply顺序写入execution/prompts，并链接vertical-integration-order；未派发、未赋写权，不增加产品计数或勾选。后续只读复审与独立开发并行，根依赖修改按a083→明确分配NFC/type任务串行。

a081运行日志称workspace解析被其他任务uncommitted oo-connectors manifest阻断，并临时用committed manifest验证后恢复。该操作不在其builder目录写权内：即使称已恢复，也不能默认没有覆盖a083更新。协调者已记录风险；其终态交接须核查具体改动/恢复时间、实际冻结输入与root/connector差异，固定archive验证不能用移动workspace测试代替。下一任务卡明确禁止临时改foreign manifest；正确做法是独立归档固定输入并只overlay自己源码。live任务不因该日志单独重启；a083仍唯一root/connector integration writer。完整集成接受前必须消除覆盖疑点。

## a081闭包修正交付 → a086固定复审

a081 handle23764实际终态0，交付eebc0ea3b835655fdb3ab088e6f7351d0d316a43（treeeebbcb4a8ff44d50c4b541d5646796a82a234210，parent79587e4）。协调者已读完整REPORT/HANDOFF，核查baseline/fixed原始Linux1.94fetch/test/Clippy/docker均0，fixed27lib+4CLI通过，五个源码blob与93c6cc57测试树一致；两process metadata记录的凭据参数marker计数0只是有限检查，不证明历史秘密gate关闭。当前foreign connector manifest SHA b9d8c7bfda4c563994024190ab0ad73c9fffefe013625a760aa47bc1f483216d仅记录观测时状态，不证明此前恢复没有覆盖。报告仍写a073 root-owner，已明确现为a083。

a086 handle15204已实际启动固定eebc0ea只读archive复审，先Linux1.94局部冒烟，再独立端点闭包/组合与自引用/重复key成员/合法复用/hash优先级/CLI脱敏负例；manifest越界操作按可见证据独立记录，不改livea083文件。a071 findings1/2/5及完整SP07继续pending；结构修复不计新功能交付，不增加已验收子项或首次复审分母，收到独立结论前不勾修正。a080/a082/a083/a084/a085上一观测仍live，不重启。Prepared三张导入任务卡尚未赋编码权。

## NFC任务准备与验证等待

上一目标回合为progress：结构修正eebc0ea已交付，e80b4f1派发a086固定复审。本回合一次实际句柄核验a080/a082/a083/a084/a085/a086全部live；对关键a083 handle30780进行单次60秒有界等待后仍live，不依据日志tool_output_error或等待超时重启。此为verified wait，不是停工或block。

v01-builder-nfc-prepared.md已补齐findings1/2/5专用任务卡：根单writer串行接入与connector相同unicode-normalization0.1.25、官方U16/U17/全标量oracle、notice/erratum、旧hash/人工决策兼容和a081回归。尚未派发且没有写权；a083当前Cargo.toml/Cargo.lock/connector变更不被协调者或其他task覆盖。任务卡准备不计产品交付/验收，不新增勾选，速度指标仍6组件/3验收子项/首轮返工2/4。真正源整合终态后先收完整证据与固定SHA再推进root任务及类型/reader/apply；所有完整父项/Phase仍pending。

## a083固定整合交付已出现，a087只读复审启动

固定交付2980e85af22c3ade05272c66c7d2835b52730322（treec027c4599cdebf701b21e97e51ded45b9067ca8e，parent1868ffb）已存在。协调者已读完整a083 REPORT/HANDOFF，重算main lock9b8f7e50d3b20042bb2d606f47276b76dd68f3568136343040fbea1ecd622057、PG日志8985b98a354cc8913218c682ad1bb25e8072b99915aac7a518c56ef91bb05dc4、完整MIT/Apache正文与来源pin匹配。main源仅整合4ec功能及六vendor/provenance差异，不覆旧421基础；实际960源语句SELECT840/WITH48/START36/ROLLBACK36，flagged0；作者final live/SDK/Clippy0、旧baseline combined101/漏contractsSDK101/Clippy缺组件1与继承worst101保留。38依赖新增、deny未跑及SDK CDLA/slashlicense/ORtoken等供应链缺口不签。

a087 handle41307实际启动固定2980e85只读复审，先Linux1.94局部冒烟，再独立mergedTLS feature guard/真实privateCA/env/权限/来源/SDK兼容与供应链范围核验。固定commit允许只读review并行，不等于a083 rootwriter终态；a087派发时handle30780仍live；随后39376ac交接核验已收到exit0，最终stamp99e00ed，root按下节转交a088。原报告称交付即释放仅属作者声明，实际写权由协调者终态交接决定。a081临时覆foreign manifest的历史不确定性单列，复审确认最终源/graph字节与实际编译/fixture，不用当前hash证明过去未覆盖。

本轮实际已有固定整合commit和新独立复审，分类progress。整合属于已交付PG组件的同一条工作线，不重复增加功能数；产品指标仍6交付组件/3验收子项/首次返工2/4，full87父项/16Phase0完成。V01-PG-01main整合、NFC纠正和所有适用全AT待验收，prepared后续任务尚未派发，不勾选完成。

## a083真实终态 → a088根依赖串行接手

39376ac交接同轮实际write_stdin返回a083 handle30780 exit0；此前日志静默不作终态，现在证据充分。最终stamp99e00ed仅交付身份记录，源2980e85不变；main Cargo.toml/lock/connector/builder tracked diff为空，其他xtask/P5未提交改动保持各自writer。完整REPORT/HANDOFF已读，源整合复审a087 handle41307持续读取固定archive，尚不勾V01-PG-01完整接收。

a088 handle98627实际启动，session eef831aa-dba4-4af2-a78b-541bc9e0a088；baseline39376ac。只接手root必要NFC依赖及builderNFC/source/tests/notices/新erratum，优先Linux1.94smoke，替换手写Unicode16实现并验证官方U16/U17及旧/新标识兼容。不写connector/vendor/SDK、a080 standaloneP5或a082 xtask/helper/harness。a086仅eebc0ea固定复审，允许与NFC实现并行；type/reader/apply仍prepared，root直到a088真实终态才再次释放。a071 findings1/2/5修复与独立复审未完成前不勾；修正既有功能不重复增交付计数。整个目标继续active，完整产品与发行门槛仍未签。

## a080/a082/a085–a089真实终态接收与下一批派发

本回合实际工具返回a08042614/a08212964/a08517385/a08615204/a08741307/a08898627/a0896967全部exit0，关闭旧handles，不再轮询。a082/a086/a088没有完整交接：分别helper编译101、独立closure探针未跑、NFC correction验证待容量；原UUID显式resume61643/91151/31015，保留未提交own overlay和原失败，不计交付或完成。NFC root写权仍只归a088，基础xtask/helper归a082；只读a086不改源码。

a084 SDK原固定复审request_changes：六项runtime为短token/短SQL/混合percent反射/长十进制Retry-After/baseURL路径/header敏感标记。原probe2 94cases中7个diagnostic false不是7个runtime漏洞，127.255 transport单列；probe2 SHA f780a4f1871266c1824290ed6eaafd785317c5ec0e6d9198d15b29dafb84fb6e已重算。smoke0、audit0、deny4仅CDLA失败，不当供应链通过。a089实际交付bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df/tree db0717173187604182b02d4fc8227b107bddb6d1/parent6831d6e，stampfc877c4；完整REPORT/HANDOFF及RED101/finalfetch-test-Clippy0已读，6lib/18HTTP/1proxy/9redaction-retry/2TLS作者通过。a09061992只读此固定提交独立六项runtime复审；nestedpercent/zeroization/生产后端cancel仍不能默认保证。

a080交付6831d6e862303a0ce1c883925068078cbcb78e23/tree2f7fc8e8bb8ea34551235190e8e200e750358af9/parent77d9577；完整REPORT/HANDOFF已读，default/hooks tests和最终Clippy0，原IPC/Clippy101保留。a09197172固定archive复审contradictoryIPC/workeridentity/所有分支时钟/真实PID-starttime清理/foreign保护及许可来源。作者proven不等最终验收；同netns/source文件可读、supervisor死亡孤儿、R4/R5/R9/OID/Gateway/runsc/amd仍pending。旧报告a073rootowner已过时，由新记录纠正，不改旧证据。

a0858370498 request_changes：真实freshgeneration上NULLindex/count1/一字节、index0/NULLcount/一字节、bothNULL/一字节均inserted；SQL CHECK和IF均接受unknown。已读完整review/findings、原始row输出并重算hash。a09296228只写pipeline前向迁移/新版本契约/tests；冻结0000/0001及v1 JSON不改，合法旧receipt不变、非法历史升级原子拒绝且无ready。schema-only receipt0/0/1、任意futureDML未fenced、installer checksum信任、owner/superuser边界继续另验。

a0872980e85 request_changes：服务端ALTER ROLE SET role使session_user/current_user不一致，pg_stat_ssl隐藏导致误报tls_not_negotiated；failclosed无注册/catalog，但未产生预期role_mismatch/session_user结构拒绝。a09362030仅connector代码/tests及fixture纠正，不改root/vendor/依赖、不发源SET/RESET或写SQL。固定merged graph/vendor/featureguard/私有CA和其余live回归通过；原缺mount/unsafe set_var/探针101保留。复审PG日志hash4fe7311b5e8c9c1fcf935d62db9d4306c639bff25e8cf7951b01519f8d685ea1已重算；源SQL仅SELECT1748/WITH102/START77/ROLLBACK77。deny4/CDLA和target-all供应链仍未签。

勘误：a084/a089重复“r-efi AUTHORS不是MIT正文”是错误表述。a078已独立证明5.3/6.0 AUTHORS含完整MIT；新erratum记录实际bodyhash，不修改历史report、不放宽basename或LGPL-orlater OR规则/CDLA。

当前产品累计交付仍6组件，独立验收子项仍3；首次产品复审6项、返工4/6=66.7%（小样本，原草案/源/SDK/目标schema返工）。a087整合复审及a090修正复审不重复计新功能first-review；a080原型和基础修复单列。87完整父项/16Phase仍0完成。本轮无新充分批准证据，因此不新增完成勾选；旧已批准子项保留。后续类型→有界reader→真实targetapply任务卡继续prepared，优先等NFC root实际交接与源role修正固定接口，基础线不阻塞独立模块。

基础a082续接handle61643再次实际exit0，无REPORT/HANDOFF/commit；attempt2 helperClippy101，builder可能仍在运行。协调者立即读终态日志后再次沿用原UUID恢复，禁止重复构建或掩盖101；其余a088/a086/a090/a091/a092/a093单次句柄核验仍live，不重启。3652115已提交最新plan/ledger/复审接收证据；文档check_plan/check_spec均0，只证完整性。原始psql stdout/cleanup格式空白保留与hash一致，格式检查限规范文档/JSON/exit不改原始输出。

## a088 NFC真实交付与根写权释放

上一轮为verified wait。a088原续接handle31015本轮实际exit0，交付3ed558a9b62ab3f290ce9674b9afc10e693a5730/tree9aec399e081177330a92f886c4c9dafaa766b4e8/parent9386766，stampbc0a2ed仅identity。协调者已读完整REPORT/HANDOFF和实际baseline/fixed各phase及docker均0，重算U16/U17 NormalizationTest/LICENSE hash匹配并检查fixedsnapshot26个builder/lock/NOTICE/contract文件0mismatch。作者27lib+4NFC+4CLI通过，Unicode16 19965rows/17 20034rows及1112064scalars断言通过；相同库调用parity不作为独立oracle批准。根manifest未改，lock仅builder两edge，手写NFCtables从当前产品移除，旧报告/生成器claim由新erratum纠正。作者audit/deny未跑仍pending。

当前root/builder tracked diff及index均空，root写权已释放；NFC不能因交付就勾findings1/2/5。a094固定3ed558a只读复审已经启动，独立官方vectors/许可/标识兼容/hashbinding及closure回归；a086结构closure另待最终结论。a093仍独占connector identity/session/tests修正，类型任务与其共享connector目录/接口，待实际终态交接后串行派发；root空闲不授权并发覆盖代码。a082foundation、a090SDK/a091P5review、a092pipeline均保留原livehandles。本次纠正不重复增加6组件/3验收子项/4比6首次返工，完整87父项/16Phase仍0完成。

## a086结构closure限定验收与基础续接

a086原续接handle91151实际exit0，协调者已读完整review/findings/HANDOFF/治理和cleanup，固定eebc0ea3b835655fdb3ab088e6f7351d0d316a43/treeeebbcb4a8ff44d50c4b541d5646796a82a234210/parent79587e4获仅findings3/4 approve。attempt1 fetch/test/Clippy0；CLI1前缀checker错误及probe101、attempt2deferred、3/4/5探针输入失败原样保留。决定性attempt6 CLI/probe/docker0，独立cases118/false0，协调者重算caseshash db5579de10fcc004bedfce83d1bbbd7c10e165932d15fd75eff6f361cd5c7250、probehash2df541e2103d91b45a3743062a4d630837dad256bf427a94356ea2a0bf2b2d16。

已仅勾选closure/repeatedkey子项，whole-dataset review.not_in_scope、未验证FK语义、physical63byte、signednumeric、NFC/完整SP07/真实import/profile/PII/publish/RBAC/CLI仍pending；a094正复审NFC3ed558a，旧eebcNFC缺陷不能当当前实现仍未修或已经批准。治理记录a081曾覆盖root和connector manifest，无savedtemp hash/恢复bytecompare，当前hash不能消除历史时序缺证；a087已经独立重建main最终graph但仍roleGUC request_changes，a093修正中。a086报告a083仍liveowner说法是旧范围叙述，实际root已在a088终态后释放，connector writer为a093，旧证据不改。

独立验收产品子项现在4（jobs schema、源三HIGH纠正、eventproject绑定、structureclosure纠正）；交付组件仍6、首轮返工仍4/6，复审纠正不重复计首次分母。87完整父项/16Phase仍0完成。a082第二续接28952实际exit0却无REPORT/HANDOFF/commit，只称attempt3launcher0待phase检查；立即原UUID第三次续接handle2404，核验原phase而非重复构建。基础线不阻塞a092/a093产品模块。其他a09061992/a09197172/a09296228/a09362030/a09424699本回合单次核验live，保留原会话。

## a095独立CLI产品线启动

前两回合为verified wait，原a082/a090/a091/a092/a093/a094当前仍live，未按静默或时长重启。本轮核查oo-cli仅占位但已依赖oo-sdk，SDKquery接口固定，CLI代码与a093connector及a092pipeline无冲突，可推进真实闭环的查询输出端。a095原生Grok4.7xhigh会话已实际启动，baseline7143b2d，onlyCLI/新contract/tests/evidence；root依赖写权由已退出a088释放后明确分配a095，优先复用已有locked依赖。禁止SDK/core/connector/其他task写入及foreignmanifest覆盖，max2全局Rustbuilder不变。

任务契约含实际oo→SDK→HTTP/TLS、SQL/-f/JSON/Markdown精度与anti-footer、错误stdout/stderr/0-2-3-4-1退出码/真实有限retry、安全显式外部credential来源，不读OO_TOKEN/home，未实现sandboxproxy提前明确拒绝；不能以独立token client或假Bearer绕过SDK。sandboxfrom_env、管理commands/config/sqlvalidate/export/discover与真正PG权限/对话/harness仍后续验收。a090SDKreview和a094NFCreview均只读archive，不阻止CLI独立开发；依赖界面发现变化须后续集成重验。此派发不算新已交付组件或完成子项，仍6交付/4验收/首轮返工4比6，full87/16仍0完成。types/reader任务等待a093实际connector目录交接，root现在CLI独占，不双writer。

## a082 Rust交付与文档漂移记录

a082第三次resume handle2404本轮实际exit0，交付f1a6005971e3dc3075094c87ed670fc9d25e9ae0/tree86c64f512d67da47ae379a53e34342de8e317f25/parent7143b2d。完整REPORT/HANDOFF已读；attempt3实际helpertests3+8、xtask39lib/16CLI、helper/xtaskClippy/probe/launch0，四advisorycurl22保留，不是auditgreen。协调者重算ELF3569f4e45f88cdedef55c6eb5b23e2c4eb9e9dc1c14690085ddf72681a95628e/716792bytes、helperlock14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1匹配。报告诚实保留forksettle/supervisorSIGKILL/sameuidcopy-exec/跨pidnamespace/cgroup0/macOSunavailable边界，不直接批准required_satisfied。a096固定f1a6005只读复审已实际启动；不签SP00/秘密/runsc/CI/release，不计产品组件和验收数。a082再次写a073rootowner是过时文字，实际rootwriter a095CLI。

本轮check_plan FAIL，原因仅docs/17-rust-implementation.md源码hash/size-lines漂移。只读diff表明是a093源roleGUC/TLSunknown说明增补，与其必要源contract/docs范围有关，sourcewriter仍live。未刷新sources-baseline、未删检查/改旧Spec hash；待a093固定交接及独立审查后协调规范文档版本/来源基线。该完整性失败继续记录为失败，不阻塞无关CLI模块，不当产品失败或验收通过。365/714时代通过结论只针对当时文档；当前没有新增文档pass声明。a095新CLI实际handle3387、a096review与其余live任务保留，global2builder不变。

## a093固定role修正出现与a097只读复审

34fae5ddd73ff250fde6f51aa681fb0f7fbd0362/treec35b95664d53dcef1497bbd224ecbeec65586cea/parent35320e9已存在，协调者已读完整a093REPORT/HANDOFF：缺pg_stat_ssl记unknown、sslfalse拒绝、实际verifyfull不降级；serverroleGUC产生role_mismatch/startup_option并拒catalog。作者最终green2 lib19/live1/Clippy0，baseline缺fixture101/REDtls_not_negotiated101/首次green缺Debug101保留，sourceSELECT843/WITH48/START36/ROLLBACK36声明。原rawSQLlogs作者hash后随TMP删除，因此2652658b...不能由保留raw重算，不当独立零写证据。新TLS字段v1兼容、unknown对alignedidentity准入、docs17安全表述须复审。

a097固定34fae5d只读archive实际启动，scope原a087role问题+maininterface实际安全，不等a093终态才能read-onlyreview。a093handle62030本轮单次wait60仍live，connector目录仍其writer；未释放或启动冲突types。root依赖仍a095CLI独占。docs17修改已进固定sourcecommit但旧sourcebaseline保持，check_plan漂移失败待独立docs语义审查后版本化处理，不删门禁、不签通过、不阻塞无关CLI。a093handoff称a088rootwriter是过时记录，实际a095；新台账纠正而不重写历史。此同源组件返工不重复增加6交付/4验收/首轮返工4比6；完整87父项/16Phase仍0完成。

## a093真实终态与a098类型解码并行启动

本轮actualwrite_stdin a093handle62030 exit0，finalstamp5e3dffe仅evidence/交付身份；源码34fae5d不变，完整REPORT/HANDOFF已读，connector tracked diff为空。源目录writer释放，不代表a097复审批准。a098现已实际启动生产PG无损类型解码任务，baselineb3a8ed3/current固定source34fae5d，消费纯canonical/Arrowmetadata/sourceinspection，覆盖真实SQLx→canonical→actualArrow及signed11bitnumeric/精度/temporal/arrays/domain/enum/JSON/typedunsupported/rowpreservation等准备卡原要求，不缩成JSON演示。

a095仍唯一mainmanifest/lockwriter；a098仅connector typecode/tests/contracts，生产connector manifest也不得改，隔离快照可用自己manifest/lock验证依赖。最终主依赖整合必须协调者在a095实际terminal+交接后显式写a098-root-writer-handoff.md授权，不能凭出现commit或闲置推断。无此授权不签最终交付；两个writer不覆盖foreignmanifest。a097源review仍固定archive，只读可与类型开发并行；新发现接口问题需后续集成修正，不代表导入准入。a098需要保留自己的sanitized rawSQLaudit，不能重用a093删除raw后的hash声明。a092target/a095CLI/诸readonlyreviews保持独立，globalmax2builder和每task1PGfixture不变。

类型任务从prepared升级in_progress，无新已交付/验收勾选，仍6组件/4子项/首次返工4比6；完整87父项/16phase未完成。root依赖a095、connectora098、pipelinea092，基础修复等待a096固定review；docs17漂移待a097语义审查，未刷新旧baseline。
