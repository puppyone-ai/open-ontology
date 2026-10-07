# OO-FE-TRACE-001 覆盖、来源与设计决定

来源快照与上位效力见 [索引](README.md)。本包遵照用户双入口决定；所有Web业务页面共享设计系统，管理/用户数据能力分离。文档成文不是实现证据。

## 1. 原Plan/Spec到前端范围

| 原工作项 | 本包页面/能力 | 原验收 | FE任务 |
|---|---|---|---|
| SP00-W01/W05/W06 | Web工具链/契约/公开fixtures/evidence/license | AT-CORE/AT-SEC-05 | FE-01/03/04 |
| SP02-W01 | jobs持久任务、checkpoint | AT-CTL-01/02 | FE-08 |
| SP02-W02/W03 | 配置/Gitdiff/validate-plan-apply | AT-CTL-03/04 | FE-06/17/19/20 |
| SP02-W04/W05 | bootstrap/OIDC/Secret/adminRBAC | AT-SEC-01,AT-CTL-05,AT-UI-01 | FE-05/17/19 |
| SP02-W06 | feedback接收/审计能力入口 | AT-CTL-06 | FE-21/25 |
| SP03-W01–W05 | PG/MySQL/类型/预算/catalog注册 | AT-SRC,AT-OOS-02 | FE-06/10 |
| SP04-W01–W06 | CDC/快照/暂停/隔离/重试/位置与质量 | AT-PL | FE-06/08/26 |
| SP05-W01–W06 | 上传/所有格式预览/策略/下载TTL/删除 | AT-FILE,AT-SEC-03 | FE-07/23 |
| SP06-W01–W06 | 授权/精确查询/限流/取消/审计/quality | AT-GW/AT-UI-02 | FE-17/22/25 |
| SP07-W01/W02 | overlay/结构画像 | AT-OOS-01/02 | FE-09/10 |
| SP07-W03/W04 | 语义规则/专家卡/实际模型候选 | AT-OOS-02/03/06 | FE-09/11 |
| SP07-W05/W06 | S9/S10/owner/角色Pack | AT-OOS-03/05 | FE-11/12/18 |
| SP08-W01 | 码表/业务日历/未知码 | AT-OOS-07,AT-DB-04 | FE-14 |
| SP08-W02 | metric认证/对账/生命周期 | AT-OOS-08/09 | FE-13 |
| SP08-W03 | 实体/keys/否定/拆分/历史 | AT-OOS-10 | FE-15 |
| SP08-W04/W05 | gate/原子发布/修复/质量 | AT-OOS-04/11,AT-PL-06/10 | FE-18/26 |
| SP08-W06 | 完整审核/语义/发布console | AT-UI-01 | FE-10–18 |
| SP09-W01–W04 | typedSDK/接口/精确export | AT-API-01–03 | FE-03/22/23 |
| SP09-W05 | 图表/分析文件manifest | AT-API-04 | FE-22/23 |
| SP10-W01–W05 | harness/预算/隔离/预热状态，不能浏览器编排 | AT-RT/AT-SEC-04 | FE-20/21/26 |
| SP11-W01–W04 | 对话/session/release/历史/outputs/网页 | AT-UI-02,AT-RT-04–08 | FE-21/22/23/25 |
| SP11-W05 | 飞书配置/验签/身份/私聊状态 | AT-UI-03 | FE-20 |
| SP11-W06 | 管理ops/初始化/安全standard启动 | AT-UI-01,AT-OPS-01/02/05 | FE-05/25/26/28 |
| SP12-W01–W06 | publicbench/customer域/compare/全用户旅程 | AT-EVAL-01–06,AT-UI-04 | FE-24/27 |
| SP13-W01–W05 | MCP/OAuth/tokenexchange/外部harness/增量建议 | AT-MCP-01–03 | FE-20/11 |
| SP14-W01–W06 | 配额/Compose/离线/备份恢复/升级/审计/发行 | AT-OPS/AT-PERF/AT-SEC-05/06 | FE-26/28 |
| SP15-W01–W07 | 可选扩展能力矩阵，后续能力独立feature | 独立ADR及相应验收 | FE-20/26（只能力展示，不假实现） |

表中AT族仅说明覆盖域，实际任务必须关联原Spec精确ID与场景；不宣称本包替代主Spec所有后端原型/测试。Self-build S0–S10映射FE-09/10/11/12/13/14/15/16；G1–G13映射FE-18；会话释放S1–S11映射FE-23/25/26。前端模型不是新释放状态机。

## 2. 来源与skill适配

| 来源 | 使用部分 | 本包决定 |
|---|---|---|
| [技术栈](../../docs/02-technology-stack.md) | React/TS/Vite/shadcn、Rust/配置即代码/许可证 | 不改用Next.js、不默认在线CDN |
| [Ontology](../../docs/06-ontology-layer.md) | S0–S10、OOS/generated-curated、owner/metric/Pack/gates | PAGE与API引用真实字段和审核语义 |
| [Agent接口](../../docs/08-agent-interface.md) | Gateway能力/SDK/receipt/精度/截断 | B09端点按真实实现逐项验证 |
| [runtime](../../docs/09-sandbox-and-agent-runtime.md) | 身份、WS、outputs、释放、渠道 | 网络EOF不假完成，历史当前权限复验 |
| [安全](../../docs/10-security.md) | 渲染/凭证/权限/外联 | SafeMarkdown、只写Secret、无外图 |
| [运维](../../docs/11-deployment-and-operations.md) | console/roles/OIDC/deploy/backup | 双入口共享身份，不引入隐含管理员数据权 |
| [配置](../../docs/12-configuration-reference.md) | source/model/harness/retention配置 | 向导映射真实Schema，Builder/Agent配置分开 |
| [评测](../../docs/13-evaluation-and-benchmarks.md) | public/customer/D/C/E/holdout | 完整报告与数据隔离，UI不捏造customer通过 |
| [Rust实施](../../docs/17-rust-implementation.md) | TS/API边界/服务安全责任 | 浏览器不直接调用DB/执行SQL管理 |

Skill本地可读出处：design-system `/Users/redux/.codex/skills/design-system/SKILL.md`（primitive→semantic→component）；ui-styling `/Users/redux/.codex/skills/ui-styling/SKILL.md`（Radix/shadcn、Tailwind、a11y）；gpt-taste `/Users/redux/.codex/skills/gpt-taste/SKILL.md`（字体层级/节制）；implement_lenis_scroll `/Users/redux/.codex/skills/implement_lenis_scroll/SKILL.md`（生命周期/独立介绍页）。路径仅说明设计来源，消费者以本包规范为公共依赖，不依赖每台机器拥有相同skill。业务页与gpt-taste强制营销AIDA/随机布局冲突时按用户的工作台用途优先，本包已冻结适用范围；不把skill营销图/scroll规则塞进表格和审批。

## 3. 官方实践参考（2026-10-08前序只读检索）

- [Cloudscape Table View](https://cloudscape.design/patterns/resource-management/view/table-view/)：列表筛选、密度、分页、空/错误状态。仅参考交互，不新增Cloudscape组件库。
- [Cloudscape Split View](https://cloudscape.design/patterns/resource-management/view/split-view/)：列表与上下文详情。
- [Carbon Data Table](https://www.carbondesignsystem.com/building-blocks/core/components/data-table/guidelines)：资源表格和批量动作。
- [shadcn Data Table](https://ui.shadcn.com/docs/components/data-table)：组合TanStack Table的方式；实施冻结Radix版本和选定锁文件，不照搬最新示例API。
- [WAI Table Pattern](https://www.w3.org/WAI/ARIA/apg/patterns/table/) 与 [WCAG2.2](https://www.w3.org/TR/WCAG22/)：语义/键盘/对比度与可访问性。
- [TanStack Query Cancellation](https://tanstack.com/query/latest/docs/framework/react/guides/query-cancellation)：明确消费AbortSignal，fetch取消≠服务器业务取消。
- [Playwright Best Practices](https://playwright.dev/docs/best-practices)：用户可见定位/隔离测试/trace。
- [Lenis官方](https://github.com/darkroomengineering/lenis)：实例/RAF管理、减少动效/嵌套滚动；本业务Shell不用全局平滑滚动。

这些来源支持工程选择，不意味着某一种UI客观“全行业最佳”。依赖精确版本、兼容性、许可证和浏览器行为由FE-01首次实证，设计候选不能直接算已验证。

字体官方来源：[Geist](https://vercel.com/font)、[Geist仓库](https://github.com/vercel/geist-font)、[Noto CJK](https://github.com/notofonts/noto-cjk)。选择300标题/400正文的本产品视觉参数，实际字重文件/字形/许可证/版本仍在FE-01冻结；不加载官方站的远程字体到产品。
