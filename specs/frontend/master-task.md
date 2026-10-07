# OO-FE-MASTER-001 — 完整前端实现、独立复审与验收总任务卡

| 属性 | 值 |
|---|---|
| 版本/日期 | 1.0 / 2026-10-08，Asia/Shanghai |
| 执行状态 | 总卡待主协调接管；不声明前端尚未开工或已经完成，实际状态以当前台账/进程/固定提交证据为准 |
| 目标 | 交付完整用户与管理Web前端，接入真实后端，通过固定提交独立审阅、真实旅程与可复现部署验收 |
| 技术栈 | React + TypeScript + Vite + Tailwind + shadcn/ui；服务与业务后端继续Rust |
| 执行者 | 最多三个固定Grok Build CLI会话，模型grok-4.7、reasoning xhigh；含开发、独立审阅、测试与验收 |
| 主协调 | Codex统筹依赖、文件所有权、固定基线、证据核验与Plan更新；不另起编码/测试智能体 |
| 派发方式 | 本总卡是唯一派发入口，G1/G2/G3按角色读取；28张小卡仅为细节/覆盖参考，不逐卡开CLI |
| 上位范围 | 完整主Spec/Plan；不缩成只有聊天或只有mock的demo |

## 1. 接管前置与三个会话硬上限

1. 主协调先读取当前 [实施台账](delivery-plan.md) 和实际已存在的前端会话状态、owned paths、提交/报告；当前台账记录FE-01已有开发线，不能因为总卡成文再次启动重复基础线。已存在基础开发会话优先归为G1；已有独立前端review优先归为G3。尚未启动的分散卡转为总卡内检查项。
2. 最多三名固定执行者/三个会话：**G1基础与用户端，G2管理端，G3独立复审与验收**。此上限包括review，不继承旧“review不占槽”的前端执行规则；主项目其他非前端任务不由本卡擅自中止或重分配。
3. 同一角色的CLI退出后恢复原UUID，既收集已运行资源/证据，再继续；不得为一次失败/编译等待/缺报告新增第四会话，也不得同UUID同时启动两个CLI。禁止nested subagents、额外Codex编码、代调用隐藏reviewer。
4. 若现有前端CLI超过三条，协调者在现有任务固定交付/安全边界处串行收敛；不杀外部任务、不清理别人资源、不假定台账即运行中。新的G2/G3只有确认总会话≤3才派发。当前侧会话只写文档，不操作运行任务。
5. 主协调填写：G1 UUID/handle、G2 UUID/handle、G3 UUID/handle、固定base/source SHA、Node/pnpm/browser版本、后端服务commit、所有权映射、资源预算、日期及未满足外部条件；字段缺失不得伪造人名/证据。

| 固定角色 | 完整职责 | 不允许 |
|---|---|---|
| G1 | 工具链/锁/字体/Token/基础组件/Shell/SDK/APIadapter/身份、用户对话/知识/结果/历史/文件、最终路由组装与前端部署制品；自测并提交 | 自签独立验收；擅改G2目录或Rust生产者；创建额外coding会话 |
| G2 | 管理端全模块：接入/文件/任务/build/ontology/review/分类/业务语义/权限/发布/配置/评测/审计/运维；自测并提交 | 改依赖锁/共享Token/SDK/router/用户端；复刻公共组件或自造后端业务逻辑 |
| G3 | 只读固定产品提交复审；编写独立必要测试/oracle；视觉/a11y/API/真实旅程/部署验证；证据与问题报告 | 修改G1/G2产品代码；review自己修的产品；扩展为第四角色/无限追加攻击矩阵 |

## 2. 必读规范与阅读顺序

以下是必须遵守的规范，不要求把规则复制进每个子报告。总卡集中规定执行方式；各参考文档继续是视觉、页面、接口的真相源。若旧卡的会话上限/分工/派发方式与本卡冲突，以本卡为准；主Spec/Plan的安全/业务/发行要求仍优先。

| 顺序 | 文档 | 用途 |
|---|---|---|
| 1 | [主Spec](../open-ontology-v0.1-spec.md)、[主Plan](../open-ontology-v0.1-plan.md) | 产品定义、原需求/AT/SP、Rust边界、工程与完整验收 |
| 2 | [前端索引](README.md)、[设计系统](design-system.md)、[内容体系](content-system.md) | 双入口、审美、字体/Token/密度、内容来源、正确状态文案 |
| 3 | [页面规格](page-specifications.md)、[公共组件](component-contracts.md) | 50页面/路由/流程/响应式/键盘/布局/组件契约 |
| 4 | [API与状态](api-state-contracts.md) | B01–B12真实绑定、身份/版本/精度/事件/失败语义 |
| 5 | [前端验收](acceptance-evidence.md) | FE-AT-01–20、视觉/a11y、真实E2E、CI/CD/发行 |
| 6 | [任务公共协议](task-protocol.md) | Linux冒烟、资源/Secret隔离、固定提交、证据格式；分派与前端CLI上限按本卡覆盖 |
| 7 | [覆盖矩阵](coverage-sources.md)、[来源哈希](source-baseline.md) | SP/AT映射、文档及skill/官方参考来源 |
| 8 | [28卡细节索引](tasks/README.md)、[实施台账](delivery-plan.md) | 检查细节及接管已有工作；不再分别派发 |

原设计必读：[技术栈](../../docs/02-technology-stack.md)、[数据源](../../docs/03-sources-and-ingestion.md)、[Pipeline](../../docs/04-pipeline.md)、[Agent DB](../../docs/05-agent-db.md)、[Ontology](../../docs/06-ontology-layer.md)、[Gateway](../../docs/07-gateway.md)、[接口](../../docs/08-agent-interface.md)、[runtime](../../docs/09-sandbox-and-agent-runtime.md)、[安全](../../docs/10-security.md)、[运维](../../docs/11-deployment-and-operations.md)、[配置](../../docs/12-configuration-reference.md)、[评测](../../docs/13-evaluation-and-benchmarks.md)、[Rust实施](../../docs/17-rust-implementation.md)。准确文件路径以仓库索引为准；发现过期引用应报告并修文档，不能略过要求。

Skill采用范围已冻结于设计系统：design-system三级Token；ui-styling/Radix/Tailwind/a11y；gpt-taste轻盈字体/视觉层级（不套营销AIDA/随机布局到审批台）；Lenis仅独立介绍页可选且正确销毁RAF，业务页原生滚动。技能不是生产依赖，消费者不要求相同本地绝对目录；完整约束见coverage来源。

## 3. 产品体验与设计规范（直接执行）

### 3.1 两入口、一体系

- `/` 是用户数据库对话入口；未认证进入 `/login`，登录后按获准project/data space/role进入对话。
- `/admin` 是管理应用入口；未认证进入 `/admin/login`，内部子路由按页面清单组织。普通用户即使直接访问地址，也必须被后端拒绝管理操作。
- 默认企业OIDC、管理面板配置用户/组到project/role的绑定；本地账号密码是另立认证契约的扩展，本任务不得自建万能管理员密码。
- 管理权限不自动包含全部业务数据读取权限；历史、预览、下载和Query每次按当前权限验证。
- Admin/Chat两布局共享Token/组件/字体/反馈/i18n/认证和SDK。官网/文档站独立，`/`不能被替换为marketing landing page。

### 3.2 高级、轻盈但可读

| 项目 | 强制基线 |
|---|---|
| 字体 | 自托管Geist；中文Noto Sans SC；技术文字Geist Mono；实际许可/version/hash由G1记录 |
| 字重 | 标题300或400、正文400、动作500；不把小号正文/警告做成低对比超细字 |
| 尺度 | 标题26/34px、区块18/26px、正文14/22px、辅助13/20px、技术13/20px；rem支持缩放 |
| 色彩 | 暖中性浅色默认、石墨深色完整支持、克制单蓝强调；primitive→semantic→component三级Token |
| 间距 | 4px基本尺度，8/12/16/24/32/48；input圆角6/panel8/dialog12；阴影只用于浮层 |
| 布局 | sidebar232px/折叠64、topbar56、桌面边距24/小屏16；表格流式宽，表单≤960px |
| 数据密度 | compact32/comfortable40/roomy48；移动触控44px，不为了密度隐藏关键文本 |
| 主题 | light/dark/system、语言zh-CN/en、密度偏好；非敏感偏好可持久，业务记录不可随意缓存 |
| 动效 | hover100–150ms、panel160–220ms；opacity/transform优先；减少动效回退；无全局Lenis/GSAP pin |
| 可访问性 | WCAG2.2AA目标，普通文字≥4.5:1、大字/必要控件≥3:1；focus可见，keyboard/200%zoom/IME有效 |
| 审美禁止项 | 大Hero、巨型数字卡堆砌、无意义渐变/发光、随机布局、装饰图谱、外部图片、全页overflow隐藏错误 |

具体颜色值/组件变体/响应式以DS和COMP为准，G2不得局部重新选颜色/字体。字体缺字有安全fallback，中文罕见字覆盖实际验证；只preload必要字体，禁止外部CDN/Google Fonts运行外联。

### 3.3 内容与真实状态

“连接成功/已导入/待审核/已审核/已认证/已发布/可问答”分别显示。模型confidence叫“模型自评置信度”；估计行数标估计；unknown≠0。金额/decimal/int8保持精确，NULL不等空串，时间含业务时区。

专家以自然语言问题/证据作决定，SQL/JSON折叠。所有warnings保留在结果和历史；query引用必须真实receipt。禁model HTML/script/iframe/SVG active/远程图片/自动link preview。错误说明影响、下一步、安全traceID；不回显Secret/provider原文/SQL栈。成功反馈明确范围，例如“审核决定已保存；尚未发布”。

## 4. 完整页面与功能范围（不能缩减）

全部50页面的精确路由、状态和交互见PAGE；下列是G1/G2工作范围核对表。页面即使某后端尚未接入也应完整实现其诚实的unavailable/read-only表现，不能删除页面后宣称完成所有前端。

| 工作包 | 页面ID | Owner | 必须内容 |
|---|---|---|---|
| 用户登录/账户 | U01/U06 | G1 | OIDC/项目角色/偏好/退出；合法returnTo/过期/跨tab清理 |
| 对话/记录 | U02/U03 | G1 | 当前role/Pack/就绪、Composer、真实harness、tools、clarification/notfound/gap、重连/取消/关闭 |
| 结果/文件 | U03/U04 | G1 | 精确table/chart、SQL/citation、freshness/quality、partial/export、下载当前鉴权/TTL |
| 角色知识 | U05 | G1 | 发布Pack目录/表/术语/指标/示例/关系；隐藏对象不泄漏 |
| 管理登录/初始化 | A01/A02 | G1 | 入口能力、一次性bootstrap、不能复用或自设万能密码 |
| 总览 | A03 | G2 | 真实阻断/待办/源/build/session/eval状态，导航到处理对象 |
| 数据接入 | A04–A08 | G2 | database连接/体检/选表/DBA/plan/apply；上传/格式/预览/映射/模式/拒绝/TTL |
| 任务和构建 | A09–A12 | G2 | jobs/checkpoint/retry/cancel、S0–S10/cards/候选/model/cache/usage/cost |
| 语义浏览 | A13–A16 | G2 | catalog/字段/PK-FK/profile、generated-curated-effective、关系局部图+表、术语 |
| 专家审核 | A17/A18 | G2 | 队列/问题/证据/改后接受/拒绝/暂缓/合法转交、actor/time/seal/revision |
| 分类 | A19 | G2 | pii/semantic_type/来源/unknown/角色；批量范围preview且不假授权 |
| 业务语义 | A20–A25 | G2 | enums有效期/unknown、calendar闭账/锚点、metric owner/control_total/语义diff、实体否定/拆分/守恒、examples验证 |
| 权限 | A26–A28 | G2 | role表列行/claims绑定/限额、validate-plan-apply、受审server preview |
| 发布 | A29/A30 | G2 | candidate/active/diff、G1–G13、rolePackmanifest、fence/replica/CAS、失败恢复/回滚 |
| 配置 | A31–A36 | G2 | Builder/Agent独立模型/Secret/预算/驻留、harness/AGENTS追加、objectstore/TTL/Git、MCP/OAuth/channel能力 |
| 评测 | A37/A38 | G2 | suites/public-private/dev-holdout-live、n/CI/成本/错误归因/C0–C3/versiondiff |
| 审计会话 | A39–A41 | G2 | 授权回放/SQL关联、actor/query/trace、session预算/终止/释放证据 |
| 运维恢复 | A42/A43 | G2 | health/lag/backlog/quality/repair、emergency-stop、backup/upgrade/rollback/真实job |
| 公共错误 | E01 | G1 | empty/no-match/error/forbidden/unsupported/unknownstate等完整体验 |

关键旅程：

1. 管理员OIDC→PG连接→只读体检→选表/预算/DBA清单→不可变plan→apply/import/catalog。
2. 真实Builder模型→S5表与字段候选→S9专家决定→S10分类→业务语义/owner认证（适用）→gates→Pack/授权/副本屏障→active版本。
3. 用户OIDC→获准space/role→真实Agent提问→实际PG结果/口径/锚点/warnings/query citation→完整或明确partial文件→结束turn/session→资源确认释放。
4. 事实变化/新列/新码→增量候选/审核→版本差异→正确新发布；unknown不得自动公开。
5. 审计/评测/运维/恢复按当前权限和正式backend能力完成；MCP/其他harness/可选扩展只能按对应阶段验证后标支持。

## 5. 后端接口与状态的完整接入

### 5.1 B01–B12逐组绑定

G1统一维护SDK/APIbinding；G2只调用冻结adapter。所有绑定记录operationID、method/path、服务commit、OpenAPI/Schemahash、capability/scope、revision/CAS、errors/events、publicfixture、实际live证据。

| 组 | 能力 |
|---|---|
| B01 | OIDC/bootstrap/identity/capability/project/role绑定 |
| B02 | connector matrix/source config/inspect/catalog/DBA/plan/apply |
| B03 | jobs/checkpoints/events/cancel/retry |
| B04 | Builder/cards/candidates/reviews/classifications |
| B05 | 术语/码表/日历/metric/entity/examples/Pack/gates/publish |
| B06 | roles/bindings/preview/clienttoken/revoke/OAuth |
| B07 | Secret只写/轮换/对象分片/complete/abort/download/delete |
| B08 | ownconversation/session/turn/event/history/restore/close |
| B09 | Gateway tables/describe/query/validate/entities/values/freshness/knowledge/export/gap/receipt |
| B10 | eval suites/run/result/privateholdout/compare |
| B11 | Git/config/models/harness/channels/deployment/plugin |
| B12 | health/audit/回放/emergency/backup/upgrade/recovery |

`unbound→schema_frozen→fixture_verified→live_verified`独立记录；无API可先publicUI，但标未接入，不能假完成。docs中的设计端点不代表已经运行；loopback实验返回501不能被前端包装成正式query能力。

### 5.2 强制接入行为

- 优先正式OpenAPI生成typedSDK；禁止any/cast掩盖schema不符，未知枚举显示不兼容而非success。
- 身份/权限由服务端认证，Token/Secret不进localStorage/VITE_变量/URL/log/screenshots；建议同源HttpOnly会话及正式CSRF/origin保护，若现有架构不同先冻结binding而非私改认证。
- cache按subject/project/currentrole/policy/knowledge/catalog版本和参数隔离；late响应不能写进新身份；退出/撤销清敏感缓存与流。
- 审核/分类/权限/认证/发布/文件完成不得乐观假成功；expectedrevision/seal/idempotency按真实backend协议；409/412保留输入、展示新旧diff、重新决定。
- fetch AbortSignal仅取消HTTP；真正job/query/harness取消使用正式服务端操作并观察确认。WebSocket按eventID/sequence/generation重连去重，EOF不是turn/export/release完成。
- int8/decimal使用字符串或coefficient/scale；图轴近似须注明，tooltip/copy/export保留精确值。Arrow导出完成meta/hash/rows一致才complete。
- Secret保存后清输入，只显示ref/configured/rotation状态；Builder/Agent独立配置；空输入保持，explicitclear单独动作。
- 后端未实现的publicauth/完整发布/runsc/harness等登记mainline依赖，不由前端自造授权/SQL/状态/伪receipt解决。Grok仅做获准公开/本地PG夹具测试，不直接访问企业测试源。

## 6. 文件所有权与无冲突并行

先核对当前仓库已有路径；以下是默认拟实施归属，已有生产者接口/锁的写入权只由协调者正式移交一次。两开发者同checkout，必须按固定source构建，不把对方dirty工作区当已交付依赖。

| 范围 | 唯一Owner |
|---|---|
| 根JS package.json/pnpm-workspace.yaml/pnpm-lock.yaml；apps/web各manifest/config/buildscripts/versions/font/license/token/storybookinfra | G1 |
| apps/web/src/app/**、layouts/**、styles/**、components/ui/**、components/shared/**、components/stories/**、api/**；packages/sdk-ts/** | G1 |
| apps/web/src/features/{auth,chat,query-results,history-artifacts,user-knowledge,account}/**，其featuretests/stories | G1 |
| apps/web/src/features/{admin-overview,sources,files,jobs,builds,ontology-catalog,reviews,classifications,metrics,enums-calendar,entities-terms,examples,permissions,releases,models-secrets,settings-integrations,evaluations,audit-sessions,operations}/** | G2 |
| 管理features同目录自有tests/stories、apps/web/tests/e2e/admin/**、管理APIBINDING消费记录 | G2 |
| apps/web/deploy/**、自身用户e2e/**、app路由/最终生产构建 | G1；既有Compose/Caddy/CI由主部署owner串行接纳，不能隐式扩权 |
| apps/web/tests/acceptance/**、apps/web/tests/review/**、specs/frontend/reviews/**、独立evidence/<G3-run>/frontend/** | G3，测试与报告，不改产品 |
| specs/frontend/execution-status.md | Codex唯一维护实际任务/提交/review/计数链接；未创建前不假称已有 |
| Rust生产者、rootCargo、Secretstore、其他mainline/evidence、真实客户数据 | 本总卡默认只读；后端缺口交原owner |

公共C01–C20由G1维护，G2只能请求新增props/状态并等固定提交；G2 domain组件留feature内，不复制基础组件。G2提供冻结ModuleRegistration/module.ts（routeID/path/capability/loader/page exports），G1唯一挂路由；SDK新增operation同样G1绑定。G3fixture/oracle不得改变G1/G2source或gold。

## 7. 里程碑与执行范式

整个任务只有三个会话，但内部仍按可验收里程碑提交，避免一个巨大dirtyworkspace才review；不是重新产生多张派发卡。

| 里程碑 | G1 | G2 | G3/退出条件 |
|---|---|---|---|
| M0 接管 | 核对已有FE-01/锁/资源，固定base/version/职责 | 核对管理API与缺口，准备publicfixtures与设计 | 只读规范/审阅范围，避免和未交付代码争编译资源 |
| M1 公共基座 | Linuxbootstrap、字体/Token、组件、Shell/SDK/identity/模块注册 | 固定接口后实现admin概览、source/files/jobs；依赖未就绪先准备feature内素材 | 固定提交局部review，公共API/设计冻结后允许并行 |
| M2 首个真实纵切 | 用户登录/chat/receipt/历史/outputs，实际Entry/harness绑定 | PG接入/build/review/classification/Pack发布页面绑定 | 真实公开PG流程、角色拒绝、精确值/引用、取消释放；未满足项明确 |
| M3 全管理内容 | 支持G2所需adapter/组件/统一路由/部署asset | 完成全部语义/权限/配置/评测/审计/运维50页中的管理范围 | 每模块fixedreview，必要边界一次验证，具体缺陷才扩大 |
| M4 全旅程与发行 | 接回问题修复、正式打包/版本/edge集成 | 修复管理问题、真实binding齐备 | 完整FE-AT-01–20/相关SpecAT、visual/a11y/性能、部署；外部门槛独立 |

每个里程碑交付后：author先跑局部验证→ownedpaths固定提交→G3以SHA review→发现缺陷交原author→修复新SHA后复审变化范围→Codex核验证据/更新TODO。G3可以流水处理固定交付，不能评审持续变化workspace；G1/G2可继续不冲突后续模块。作者自测和G3独立证据分目录/run。

开发前Linux已有基线冒烟，Rust变更（若另获ownership）跑MSRV1.94局部；首次前端不存在则只允许先最小manifest/lock bootstrap并如实记录首次创建，不能假称已跑不存在基线。Node/browser资源沿用已冻结全局2/2、task最多各1，Node建议4GiB1CPU；G3也占资源预算，无第四builder避上限。Rustbuilder全局10、task最多1；PG task最多1，不共享其他任务资源。实测内存不足先串行，不prune外部容器。

主协调沿用主线程等待规则，一次有界等待后统一实际handle观察，退出立即收完整报告；不持续读live logs/轮询、不因时间到重启、不给同UUID重复资源。terminal缺报告先恢复同会话收已启动proof。

## 8. 开发与测试要求

局部必跑：Linux固定Node/pnpm锁安装、TS strict、lint、build；变更组件行为和必要Playwright/axe/视觉。固定版本由G1实际选择记录，不用@latest新起依赖。复用经过审阅的环境/fixture/报告结构/recipe来源path+commit+hash，证据独立。

测试主要验证用户动作和真正状态：不写镜像实现的冗余测试，不为排版简单修改重复PG/security全矩阵。review限定必要权限、精度、版本、取消、渲染边界，新增具体失败才扩展。API fixture/MSW只证明UI，live旅程需正式后端/身份/receipt/资源证据。

视觉截图固定1440×900、1280×800、768×1024、390×844；light/dark、comfortable/compact、长中文/长数、loading/empty/error/forbidden/stale/conflict/submitting。字体已加载、clock/data固定、差异人工审，不自动接受snapshot更新；200%zoom、keyboard/IME/触控、reducedmotion实际测。

性能初始目标按验收规范：首屏公共JS gzip≤250KiB、latinfont≤120KiB、路由懒加载；具体环境budget首次实测冻结，超出写原因/拆分，不删功能通过。10万PG为服务端真实数据规模，不一次下载10万行到UI；页面受控分页/渲染，图表与关系局部展开。

## 9. 完整验收清单

每项需真实证据才勾选；下列是总卡完成条件，不是已完成状态。原FE-AT的输入/判定/Spec映射见验收规范，G3不可自行缩小范围。

- [ ] A01：两入口OIDC/bootstrap/能力路由/普通用户admin拒绝/退出/过期/项目切换正确（FE-AT-01）。
- [ ] A02：数据库向导完整真实PG接入、只读体检/DBA/plan/apply与新鲜度/预算/错误（02）。
- [ ] A03：全部声明支持文件格式解析预览/策略/导入/partial/abort/checksum/TTL/授权（03）；未支持明确且不能签全格式完成。
- [ ] A04：实际模型Self-build表/字段候选、输入hash/cache/usage/cost、失败无审核发布副作用（04）。
- [ ] A05：专家真实决定、actor/time/revision/seal/Git审计、stale/并发/owner权限（05）。
- [ ] A06：分类批量范围、PII/semantic_type/unknown隐藏、私人显式授权（06）。
- [ ] A07：术语/码表/日历/实体/示例、历史/unknown/否定/拆分/守恒正确（07）。
- [ ] A08：metric SQL/test/anchor/精确control_total/语义diff/真实owner认证正确（08）。
- [ ] A09：role/claims/预览/clientrevoke与旧下载当前权限真实执行（09）。
- [ ] A10：G1–G13、未运行不pass、不可豁免安全gate、Pack/屏障/CAS/恢复无半发布（10）。
- [ ] A11：真实Agent/harness/PG回答、精确结果/NULL/截断/口径/锚点/warnings/query citation（11）。
- [ ] A12：WS重复/缺口/重连/EOF、取消与S1–S11释放确认无假closed（12）。
- [ ] A13：历史/outputs降权/换人/过期/真实下载与恶意Markdown/XSS/外联拒绝（13）。
- [ ] A14：Builder/Agent独立配置、Secret只写与轮换、Gitvalidate-plan-apply、harness/channel能力（14）。
- [ ] A15：公开/客户/holdout评测隔离、实际n/CI/cost/error/version报告（15）。
- [ ] A16：审计回放/ops/受控repair/emergency/backup/upgrade/recovery实际状态与证据（16）。
- [ ] A17：50页面和C01–C20视觉/主题/密度/响应式/完整states、无遗漏（17）。
- [ ] A18：keyboard/焦点/读屏抽样/对比度/zoom/IME/reducedmotion实证（18）。
- [ ] A19：本地10万行PG完整Web注册→同步→构建→审核→发布→问答→文件→关闭，独立gold与释放证据（19、AT-UI-04）。
- [ ] A20：fresh Linux部署、深链/API/WS routing、离线assets/fonts、SBOM/NOTICE/Secret扫描、双架构/升级恢复（20）。

UI开发、实际API绑定、完整集成、发行准备四栏分别记录。真实owner/customer/protectedCI/runsc等外部条件保留相应gate，不阻塞无关公共UI，但缺一项不能宣称完整产品交付。模拟auth、可信operator CLI、HTTP200、模型列表、截图或manifest均不得替代对应全范围证据。

## 10. CI/CD与发布责任

1. 每里程碑Linux锁安装/types/lint/build与变化范围测试，typedSDK schema diff/生成一致性检查；secret/license检查在依赖或敏感表单变更时运行。
2. 视觉diff和交互review固定SHA，失败保留rawexit/trace，不批量接受基线或修改gold来变绿。
3. 正式集成节点运行actualauth/本地PG10万/Builder/S9S10/publish/Pack/harness/export/close，双架构只在集成/发行节点，不每页面重复。
4. Vite静态产物由既定edge提供；SPA fallback只页面深链，API/OIDC/WS不能落index.html；fonts/assets本地，index/currentidentity/config cache按权限控制，VITE_配置不含密钥。
5. G1提供web部署片段，主部署owner串行合入Compose/Caddy/protectedCI；本卡不隐含公网发布、push、PR、部署或修改非ownedinfra授权。发行审批仍主Spec/Plan。

## 11. 交付物与固定报告

- 可运行 `apps/web` 双入口、全部页面与公共设计系统；`packages/sdk-ts` 正式SDK、binding登记和必要liveadapter。
- 固定JSlock/Node/pnpm/browser/font版本、licenses/hash/SBOM/NOTICE、localassets、生产构建与部署片段。
- 组件stories、publicfixtures、各自局部测试；G3独立review/acceptance tests与真实证据。
- 每run `evidence/<run-id>/frontend/<milestone>/REPORT.md`、`FINDINGS.md`、`HANDOFF.md`、source/artifact SHA256manifest、rawexits、必要screenshots/trace/cleanup；失败尝试保留，客户证据私有。
- APIbinding明确B01–B12各operation的unbound/schema/fixture/live状态及commit；所有50页的开发/live状态、主SP/AT、fixedsource/review/evidence链接。
- Codex集中维护一份execution-status（创建前先查已有同用途台账，唯一来源）、里程碑TODO及主Plan对应切片，不往28张旧卡重复追加状态；旧卡只保留参考。

G1/G2每可验收模块立即只提交ownedpaths；共享Gitindex有别人暂存则交协调者，不能reset或混提交。G3review用固定源码归档/独立资源，不新建worktree。报告未齐、sourcehash不匹配、任务退出0但实际proof失败一律未验收。完整任务只有A01–A20全部证据及适用发行门槛满足才可done，未满足继续原会话/依赖线而不重定义目标。

## 12. 主协调转交提示词

> 请将前端执行切换到specs/frontend/master-task.md这一张总任务卡，先核对并复用已有前端会话，最多三个固定Grok Build CLI grok-4.7 xhigh会话（G1基础与用户端、G2管理端、G3独立复审与验收，审阅也计入上限），按总卡的文件所有权与里程碑推进完整50页面、真实后端绑定及10万行PG完整Web旅程，保留原Spec/Plan全部验收门槛、Linux冒烟与固定提交证据并同步TODO，原28张卡仅作细节参考，不再逐卡派发。
