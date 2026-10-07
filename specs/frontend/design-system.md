# OO-FE-DS-001 前端设计系统与产品体验总规范

版本1.0；2026-10-08；实施设计基线。入口与来源见 [索引](README.md)。MUST/SHOULD/MAY 含义继承主 Spec；规则编号供页面/任务卡直接引用。

## 1. 产品与使用者

FE-DS-01：Ontology 是 Agent 数据库上下文层；实体/关系图只是浏览视图，不能引入知识图谱存储、RDF/OWL 或绕过数据库权限。第一方用户旅程完整覆盖接入、同步、构建、业务审核、分类、权限、发布、问答、文件、关闭、评测、运维。

FE-DS-02：一个 React 应用、共享组件和设计系统、两种布局。`/` 是业务用户对话空间；`/admin` 是管理空间。未登录各自进入面向该入口的登录界面；认证可共用企业 OIDC，成功后再次检验入口能力。获得普通用户会话不等于拥有管理能力；管理权限不隐含业务数据读取权。

| 主体 | 首选首页 | 主要任务 |
|---|---|---|
| 业务用户 | `/` | 在获准项目/数据空间中问答、阅读引用、导出与历史 |
| reviewer | `/admin/reviews` | 专家卡、描述审核；不能越权认证指标或授权 |
| steward | `/admin/ontology` | 语义编辑/审核；指标认证必须同时具备该指标 owner 资格 |
| admin | `/admin` | 数据源、角色、Agent 设置、发布 |
| owner | `/admin` | 全部管理能力，含 Secret 管理与紧急停止；Secret 值依旧不可回读 |
| auditor | `/admin/audit` | 获准审计/会话只读回放 |

默认企业 OIDC；管理台定义用户/组到角色与项目的绑定。创建本地账号密码属于另行冻结的认证扩展，本包不擅自实现。一次性 bootstrap 使用既有安装契约，不创造长期万能管理员密码。

## 2. 两个版面与完整内容范围

用户版：历史对话侧栏、项目/数据空间选择、当前权限版本、消息区、输入区；结果详情采用侧面板/全屏详情。管理版：固定项目与环境、分组导航、页面标题/动作、资源内容、上下文证据面板。可导航切换的管理入口只向有能力者显示；后端必须独立拒绝未授权请求。

| 内容域 | 必含对象 |
|---|---|
| 数据接入 | 连接、凭证、TLS、只读体检、选表、DBA清单、计划、应用；上传、格式/映射/解析预览 |
| 数据生命周期 | 快照/CDC/批量模式、位置/水位、新鲜度、拒绝、配额、恢复、质量与对账 |
| Self-build | S0–S10、catalog/profile/card、LLM候选、来源/置信度/哈希、缓存/usage/成本 |
| Ontology | 数据集、字段、关系、实体、术语、码表、日历、指标、示例、generated/curated/effective |
| 审核与发布 | S9专家队列、S10分类、owner证据、差异、G1–G13、角色Pack、版本/回滚 |
| 权限与配置 | 表/列/RLS、组绑定、授权预览、Secret、Builder/Agent独立模型、harness/预算/渠道 |
| 用户服务 | 对话、工具进度、SQL/真实引用、结果/警告/时间锚点、文件/历史/恢复/关闭 |
| 管理服务 | benchmark、客户dev/holdout/live、Git配置、审计、会话、jobs、健康、备份恢复/升级 |
| 后续扩展 | MCP/OAuth客户端、其他harness/连接器、K8s；按既有M2/后续阶段能力展示 |

官网/文档站是独立介绍空间，不占 `/` 对话入口。飞书/Slack属于渠道适配，不复制管理Web；嵌入对话可复用用户组件，但身份与数据能力仍由服务端绑定。

## 3. 审美与字体

FE-DS-03：视觉关键词是轻、精细、秩序、留白、真实。默认暖中性浅色，完整石墨深色。强调色只用克制蓝色。禁止营销式Hero占满管理页、随机重排工作区、巨型指标卡堆砌、无意义渐变、循环光效、装饰性图谱和背景远程图片。

| 字体角色 | 冻结字体 | 用法 |
|---|---|---|
| 拉丁文字 | Geist | 标题300/400，正文400，动作500，避免默认系统字体成为产品主要字体 |
| 中文 | Noto Sans SC | 标题300/400，正文400，动作500；配合Geist的实际中英混排校对 |
| 技术文字 | Geist Mono | SQL、路径、字段、版本、哈希、代码；400 |

字体WOFF2自托管，记录版本/hash/许可证/OFL要求，保留可用系统fallback；禁止Google Fonts外联。中文使用固定构建的unicode-range子集加完整字形fallback，不能删掉企业姓名/罕见字；无字形必须回退。`font-display: swap`，只preload必要拉丁和当前语种子集，测量字体加载布局偏移。300仅用于足够字号的标题，不把正文/数字/警告做成难读的超细200。细腻感来自字距、行高和层级，不降低对比度。

| 角色 | 字号/行高 | 字重与规则 |
|---|---|---|
| 产品页标题 | 26/34px | 300或400，通常一行，最多两行 |
| 区块标题 | 18/26px | 400，长中文不强加字距 |
| 正文/表格 | 14/22px | 400；正文宽度约60–80个拉丁字符，中文实测 |
| 次要文字 | 13/20px | 400，仍满足正文对比度 |
| 元信息 | 12/18px | 400，仅时间/版本/标签；不能承载重要错误 |
| 业务数值 | 14–24px | 400，tabular-nums；准确值允许复制 |
| SQL/代码 | 13/20px | 400；长行局部横向滚动 |

## 4. 三层Token

FE-DS-04：raw值只允许写在Token定义；组件必须引用语义/组件变量。单位以rem转换支持缩放，下面px是设计基线。Token未来产物 `apps/web/src/styles/tokens.css`，设计源 `apps/web/design/tokens.json`；本包不创建代码。light/dark/system和comfortable/compact全部经同一语义映射。

| Primitive | Light候选 | Dark候选 | 语义 |
|---|---|---|---|
| neutral-canvas | #F7F8FA | #101318 | canvas |
| neutral-surface | #FFFFFF | #171C24 | surface |
| neutral-subtle | #F0F2F5 | #202733 | surface-subtle |
| neutral-text | #202733 | #EDF0F5 | text-primary |
| neutral-muted | #566170 | #A8B2C2 | text-secondary |
| neutral-border | #DDE2EA | #364151 | border-subtle |
| blue-action | #245BD6 | #8EB0FF | accent/text-link；按钮背景/前景需单独映射 |
| success-text | #176B47 | #80D8AD | success-fg |
| warning-text | #8A570A | #F0C36C | warning-fg |
| danger-text | #B42332 | #FF9EA7 | danger-fg |

这些是待实测对比度的设计值，不是已通过WCAG的证据。文本普通≥4.5:1、大字≥3:1、必要控件边界/焦点≥3:1；装饰边框可以更淡，但不能成为唯一识别方式。深色主按钮必须明确dark-on-light前景，不能共用白字映射。状态背景采用独立低饱和Token，禁止直接降低文字opacity。

尺度：space=4/8/12/16/24/32/48；radius=input6/panel8/dialog12；border1px；shadow只浮层和dialog；focus2px+offset2；z=base0/sticky10/popover30/dialog50/toast60，禁止局部随意9999。常用组件语义：surface、surface-selected、text-primary、text-secondary、border、focus、accent、success、warning、danger；组件层：button-primary-bg/fg、input-border/error、table-row-height、review-evidence-bg、chat-message-max-width。

## 5. 布局与响应式

FE-DS-05：管理侧栏232px，可收窄至64px；顶部56px；内容边距桌面24/小屏16。主区域流式宽度，表格可充分使用宽屏，编辑表单最大960px，长文说明最大800px。每页只有一个主要动作，次要操作在工具栏或更多菜单。筛选栏与表头sticky，不能遮挡焦点。

| 宽度 | 管理版 | 对话版 |
|---|---|---|
| ≥1440 | 主导航+列表/编辑+证据，三栏；队列280/证据320px可调 | 历史240px+消息，详情可并列 |
| 1024–1439 | 两栏，证据抽屉；侧栏可折叠 | 历史可折叠，结果详情覆盖式 |
| 768–1023 | 内容单主栏，证据/导航抽屉 | 单栏消息与结果独立详情 |
| <768 | 单栏列表→详情，操作底部固定但不遮内容 | 输入适配软键盘/safe-area，44px触控操作 |

横向滚动只允许在数据表/代码/关系画布的明确容器；不能用全局overflow-x-hidden掩盖布局/可访问性问题。移动端不隐藏关键证据、拒绝原因和警告。所有数据密度有compact32/comfortable40/roomy48行高，但移动触控目标仍44px；键盘焦点有独立可见环。

## 6. 内容与状态语言

FE-DS-06：默认zh-CN，结构化i18n覆盖en；机器标识保持原名且可复制，业务label优先。术语表固定：草案/待审核/已审核/已认证/已发布/已弃用，不能合成一个“完成”。模型自评置信度、估计行数、未知成本、数据截至、当前角色必须准确命名。未知值用“未知”，0只能表示实际0；未运行/失败/无权限各不相同。

数字：精确decimal不转Number；金额有单位/币种，负数保持语义；时间显示业务时区和明确日期，hover/详情给绝对ISO时间；相对时间不能作为唯一证据。行数与覆盖率显示分母/估计说明。成功提示指出真实完成对象，例如“审核决定已保存，尚未发布”。错误包含发生什么、影响、下一步、trace_id；不展示数据库栈/Secret/原始敏感SQL错误。

未经审核文案有标签且不冒充事实。Markdown禁脚本/iframe/外图/自动链接预览/任意HTML；引用只有服务端验证的query_id可以成为可点击证据。空白成功态禁止：空资源、无筛选结果、无权限、API未绑定、网络失败各有组件和明确行动。

## 7. 动效与skill适配

FE-DS-07：业务界面使用CSS短动效，hover100–150ms、抽屉160–220ms、opacity/transform优先。行选择、审核提交与危险操作不可延迟等待动画；按钮不缩放引起位置移动。后台更新不能重排专家正在编辑的条目。

`gpt-taste`用于字体、节制、布局层级、清晰按钮；其强制AIDA、Hero、随机布局、超大间距、图片、GSAP pin/卡片堆叠是营销页范式，与本产品工作台目的冲突，本包明确不应用到控制台/对话。不是让Grok凭审美自由随机化。官网如后续单立任务可按该skill执行真实设计探索，不伪造Python随机运行结果。

Lenis不全局安装到业务Shell；表格、代码、对话、Dialog、ReviewPanel默认原生滚动。独立介绍页若使用lenis，必须单一RAF或单一GSAP ticker（不能两者重复）、卸载取消RAF/ticker/监听、销毁实例、减少动效回退、验证锚点/历史恢复/嵌套滚动；不直接复用skill未取消外部RAF的示例。GSAP不是本控制台强制依赖。

## 8. 工程实现基线

FE-DS-08：沿用React+TypeScript+Vite+shadcn/ui、Tailwind；TS strict。选定单一shadcn基础实现（Radix），不混用Base/Aria示例而不审查。建议冻结React Router、TanStack Query/Table、React Hook Form+Zod、Storybook、Vitest/Testing Library、MSW、Playwright；版本/NodeLTS/包管理器pnpm在FE-01的Linux安装/build证据后写入versions记录与lock，不在文档猜最新版本。Graph推荐React Flow/ELK作为待审依赖；图默认小范围、按需加载、只读。Charts优先固定Recharts并保留等价数值表。

应用根 `apps/web`；真实TS SDK根 `packages/sdk-ts`（若主进程已有路径则在FE-01记录唯一迁移决定，禁止第二套SDK）。JS workspace统一使用仓库根package.json/pnpm-workspace.yaml/pnpm-lock.yaml，包位于apps/web与packages/sdk-ts；若已有JS工作区则先登记复用，不能创建第二锁文件。所有npm清单/锁/构建脚本由FE-01唯一拥有，其他页面卡不编辑依赖。公共路由映射只有FE-02拥有；模块注册接口冻结后，各任务通过自有module.ts提供内容，由Shell owner串行挂接。

FE-DS-09：OpenAPI/JSON Schema是真实类型源；页面不复制Rust业务规则，不直接访问DB/Docker/Git/Secret源文件，不承载Builder/Agent密钥。服务端变更后的返回版本决定UI；本地mock只开发展示，显著DEMO且不能进入真实集成验收。HTTP取消不等于PG/harness取消；网络断流不等于turn/export/release完成。

FE-DS-10：用户/project/role/policy/knowledge版本参与适用cache隔离；换身份/退出清缓存；敏感查询不写localStorage/IndexedDB。只保存主题/密度/列宽等非敏感偏好且按身份命名空间。表格服务端分页默认50行；大数据不下载到浏览器扫描，虚拟化只在真实性能需要时用，保持读屏语义与可复制值。

## 9. 交付条件

每个页面交付真实布局+全部公共状态+权限拒绝+必要任务状态+截图+API绑定记录；组件交付Storybook states而非只有happy path。功能、集成、发行分别验收。公共mock组件可以并行，发布/审核/auth必须等正式后端契约绑定；缺外部owner/customer/protectedCI/runsc单列，不阻塞无依赖的公共界面实现，但不能将模拟进度计作真实产品完成。具体见 [页面](page-specifications.md)、[API](api-state-contracts.md)、[验收](acceptance-evidence.md)。
