# OO-FE-AT-001 前端验收、审阅与CI/CD

v1.0；本文件是实施要求，不是已经执行的前端测试报告。继承Spec AT-UI-01–04、AT-SEC-03、AT-API-01/03、AT-OOS、AT-RT 与原发行门槛。任务卡不复制这些规则。

## 1. 三类验收

| 层次 | 可证明什么 | 必需证据 | 不能推导什么 |
|---|---|---|---|
| UI开发验收 | 组件/页面按规格运行 | fixed SHA、本地fixture交互、states/screenshots、TS/build/lint | 实际OIDC/API/PG/发布已可用 |
| Live绑定/集成 | 正式服务端行为在UI正确呈现 | Schema/commit绑定、actualauth角色、实际request/job/query/decision/receipt | 完整所有页面/客户UAT/发行 |
| 整旅程/发行 | 合规端到端和可复现部署 | Spec完整相关AT、正式依赖、双架构/离线/运维/owner/客户/CI证据 | 无证据的其他功能或SP15扩展 |

28 FE任务默认全部planned/未验收，不修改主Plan既有计数。UI组件通过≠父项done。公共数据开发不被客户外部数据阻塞，但客户验收继续pending_customer。实际runsc/publicauth/protectedCI/真实owner/客户holdout等缺失必须列明，不制造签字。

## 2. 冻结验证矩阵

| ID | 输入/动作 | 成功判定 | 原Spec |
|---|---|---|---|
| FE-AT-01 | 两入口登录、深链、普通身份访问admin、过期/退出/项目切换 | 服务端401/403、能力路由一致、旧cache无残留、returnTo合法 | AT-UI-01,AT-CTL-05 |
| FE-AT-02 | publicPG新增连接/体检/选表/DBA/plan/apply、错误TLS/版本/权限 | 所有阶段真实job/config版本，只读源、unsupported明确 | AT-UI-01,AT-UI-04 |
| FE-AT-03 | 上传预览/import、断流/checksum/配额/abort/TTL | 分片/对象/导入状态不混淆，取消与删除有实际确认 | AT-FILE-01–06（按实际映射） |
| FE-AT-04 | Self-build真实模型/publicfixture、cache、失败 | S0–S10可查，label/字段/证据真实、usage未知不伪成本、失败无审核发布副作用 | AT-OOS-01/02/06 |
| FE-AT-05 | reviewer审核、编辑/拒绝/暂缓、stale/并发决定 | actor/time/hash/revision/Git或控制审计可追踪；未授权认证/授权后端拒绝 | AT-OOS-03,AT-UI-01 |
| FE-AT-06 | 列分类、批量确认、个人/敏感列、未知新列 | 明确范围，权限不自动扩大，unknown隐藏 | AT-OOS-03,AT-GW-01 |
| FE-AT-07 | 码表/日历/实体/术语/示例真实改变 | unknown与历史有效期准确、owner资格、否定/拆分/引用生命周期正确 | AT-OOS-07/09/10,AT-DB-04 |
| FE-AT-08 | metric exactdecimal/control_total/anchor/literaldiff/错误owner | 对账/可执行/认证/发布分开；失败不能认证 | AT-OOS-08/09 |
| FE-AT-09 | role/bindings/preview/token撤销与旧下载 | 真实后端范围/audit、跨project拒绝、当前tuple重验/撤销SLO | AT-UI-01,AT-GW-01,AT-FILE-05 |
| FE-AT-10 | 发布候选、未运行gate、安全失败/合法exception、断线/CAS失败 | G1–G13实际报告、active正确、fail保持封闭；无万能force | AT-OOS-04/11 |
| FE-AT-11 | 真实Agent question→tools→SQL→rows/citations/warnings | 当前用户角色/真实receipt，precision/null/truncated正确，假引用拒绝 | AT-UI-02,AT-SEC-03 |
| FE-AT-12 | WS重复/缺口/重连/EOF/abort/关闭session | sequence去重，不重复发prompt；EOF不completed；实际取消/释放状态 | AT-RT-04–08,AT-UI-02 |
| FE-AT-13 | 历史/文件恢复、换人/降权/过期URL、恶意Markdown/SVG/link | 无跨人访问/外联/XSS，旧高权限内容拒绝、输出完整性 | AT-UI-02,AT-SEC-03,AT-RT-05 |
| FE-AT-14 | models/Secret/Git/harness/channel配置/测试 | Secret只写不泄漏，Builder与Agent独立，validate-plan-apply/audit | AT-SEC-01,AT-CTL-03/04 |
| FE-AT-15 | public/customer/holdout evaluation+版本比较 | 数据域/权限/score/n/成本/真值隔离准确，mock非customer验收 | AT-EVAL-01–06 |
| FE-AT-16 | 审计/ops/emergency/backup/recovery | 有权限可实际操作；查询取消/资源不存在/恢复证据而非动画 | AT-UI-01,AT-OPS-05/06/07/08,AT-RT-08 |
| FE-AT-17 | 全组件主题/密度/长中文/精确数值/响应式 | 符合DS/COMP，关键状态不截断，无全页横向溢出 | 前端视觉切片 |
| FE-AT-18 | keyboard/读屏/200%zoom/reducedmotion/IME/touch | WCAG2.2AA范围测试；无不可达操作/焦点遮挡/中文误发送 | 前端可访问性切片 |
| FE-AT-19 | 10万PG本地夹具完整UI旅程 | 实际source→catalog→Builder→审核分类→发布→Pack→Agent问答→文件→close；gold与释放报告 | AT-UI-04,SP12-W06 |
| FE-AT-20 | Linux部署fresh/refresh deep-link/双架构/离线/升级 | 固定fonts/assets/lock、无外部CDN、API无HTML fallback、真实rollback | AT-OPS-01/03/04/07,AT-SEC-05 |

原AT映射需实施者核对具体子编号，尤其AT-FILE/OPS不能凭此表假称全系列通过。实际验证范围在manifest列出精确AT与原Spec条目；未覆盖明确not_run。

## 3. 视觉与可访问性证据

固定Chromium/browser版本、locale=zh-CN、timezone=Asia/Shanghai、字体加载完成、synthetic deterministic data、日期clock/animations控制。截图矩阵1440×900、1280×800、768×1024、390×844；关键页面light/dark、comfortable/compact；全部共有states通过Storybook覆盖，页面代表性长中文/空/失败/冲突/权限逐项覆盖。截图提交基线必须人工复审，不能自动接受所有diff；固定SHA记录。

视觉检查：字体确实加载而非fallback；标题300/400可读；对比度实际测量；sticky不遮焦点；tablet/mobile关键操作可达；money长数/NULL/unknown/时间显示无截断歧义。axe零未处理严重/致命问题；自动axe不能证明WCAG全面，另外跑键盘、读屏抽样、200%缩放、focus return、中文IME、触控和reducedmotion。取舍须记录影响及必要后续，不删warning只追分数。

## 4. 工程性能目标（待测，不伪证）

首次FE-01冻结测试机器/网络/缓存与实际bundle baseline；首轮预算建议：首屏公共JS gzip≤250KiB，业务重编辑器/图表/图按路由加载，首屏latinfont≤120KiB，中文子集/完整fallback按实际测量列明。超过预算提交构成与合理拆分方案，不靠删除必要功能通过。CSS/JS/font预算独立。

目标：选定内网冷加载场景LCP≤2.5s、CLS≤0.1；交互INP p75≤200ms需足够实际样本，不能把一条实验室值称现场p75。10000metadata对象服务端分页、最多受控渲染行；10万PG总行数不是页面加载分母。列表/编辑连续使用15min与30次路由切换检查未释放WS/RAF/listener、重复fetch及明显内存增长；未获证明不宣称零泄漏。

## 5. CI与CD

| 节点 | 检查 | 范围原则 |
|---|---|---|
| 每PR Linux冒烟 | 固定Node/pnpm install --frozen-lockfile、TS strict、lint、production build；版本脚本FE-01交付 | 禁在线@latest、非锁更新 |
| 组件/页面PR | 变化组件行为、必要axe/截图/Playwright，typed SDK契约 | 不重复所有后端/双架构 |
| 后端绑定PR | OpenAPI diff/生成一致性、golden schemas、scope/401/403/409、实际liveverticalslice | 不能只有MSW |
| 集成节点 | 正式auth/publicPG10万/实际Builder/harness/publish全旅程、取消释放、角色切换 | 完整AT跨模块，不改fixture避缺口 |
| 发行节点 | Linuxarm64/amd64 standard Compose、deep-links、离线assets/字体、SBOM/NOTICE/CSP/Secret扫描、升级恢复 | protectedCI/owner/runsc/customer外部签收独立 |

测试脚本名由FE-01固定为package.json真实scripts（建议check/types/lint/test/build/test:e2e/test:visual/test:a11y），task不能声称脚本已存在。CI先产物后审核；视觉差异有review，生产部署/发行遵守主goal授权与门槛。Preview用publicdata隔离环境，Storybook不默认部署到生产；截图/trace/redactedHAR禁止Secrets/customer内容。CSP与Markdown策略用实际测试，不能仅配置header自称无XSS。

## 6. 部署集成

Vite产物由既定edge/Caddy提供；`/admin/*` 与用户深链SPA fallback仅适用于页面，`/api`/`/v1`/OIDC回调/WS不能落index.html。确认实际服务路径后配置，别在文档假定已有/api。所有fonts/icon/assets自托管，fingerprinted静态长cache，index/identity/config遵守短cache/no-store；WS代理升级、idle时间与产品会话时限分开。不得直接打包管理Secret/数据库连接串到VITE_变量。

OIDC logout/401跨tab事件清理敏感cache；部署basepath/callback/public origin固定校验；错误页面不暴露内部路由Secret。离线制品含前端/字体license/hash、Nodebuild固定信息；构建机可用npm与客户离线运行是不同阶段。

## 7. 证据包和审阅

目录 `evidence/<run-id>/frontend/<FE-ID>/`，开发与独立review用不同runID。文件：REPORT（范围、固定source、实际命令/exit、图/接口/能力证明）；FINDINGS（缺陷、限界、not_run）；HANDOFF（依赖/重现/owner资源/cleanup）；manifest（artifact/source/SHA256/工具版本/环境、对应AT、公开或私有域）。必要截图、trace、rawexits不覆盖失败尝试；tokens/密码/customer gold不存公共包。

设计review检Token/排版/组件/内容；功能review按任务卡固定输入/输出；API/security review只必要权限/精度/状态边界；集成review检完整旅程和主Plan。修复/复审不增加开发计数。协调者分别更新FE开发、livebindings、整旅程、发行状态，不因大部分页面都有mock而将父项勾done。
