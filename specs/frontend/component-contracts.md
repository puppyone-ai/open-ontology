# OO-FE-COMP-001 公共组件契约

继承 [DS](design-system.md)；组件只负责表现/交互，业务权限与状态来自正式API。公共维护者负责API变更，页面任务不能另造同名组件。

| ID / 组件 | 输入契约 | 交互及状态 | 验收重点 |
|---|---|---|---|
| C01 AppShell | entry、identity、project、capabilities、module routes | Admin/Chat/Auth独立layout，focus/scroll恢复 | 普通用户无admin入口；深链登录返回；小屏drawer |
| C02 PageHeader | title、description、version、primary/secondary actions | action按capability禁用并解释；一个primary | 两行标题/长中文/200%zoom不截关键文字 |
| C03 ResourceTable | stableID、server page/sort/filter、count exactness、selection scope | 语义table，URL筛选、列设置、粘性表头、三密度 | 空/无结果/失败不同；切页选择规则明确 |
| C04 StatusBadge | typed domain state、label、reason | 色+文字+必要icon，不显示万能green | pending/not_run/unknown≠0/pass |
| C05 EvidencePanel | immutable evidence ref/hash/time/provenance、authorized snippets | 自然语言摘要、可展开technical、局部滚动 | 不能回显隐藏样本/任意HTML |
| C06 ReviewDecisionForm | candidate/ref/version、allowed decisions、draft input | 接受/改后接受/拒绝/暂缓、理由、提交/冲突 | 提交后才更新；stale重新决定；input保留 |
| C07 DiffViewer | structured before/after、semantic affected refs | side-by-side/unified、小屏切换 | 字面量/scale/role差异不归一消失 |
| C08 GateList | gate IDs、pass/block/warn/not_run、report/hash | 简短概览→详细报告→合法exception | 不提供安全force；未运行不可green |
| C09 JobTimeline | job generation/seq/checkpoints/deadlines | running/reconnecting/failed/confirmed；可用动作 | EOF≠完成；估计进度注明分母 |
| C10 PermissionPreview | server principal+role+tuple、visible/denied objects | 只读受审preview，审计说明 | 不能浏览器自行过滤仿授权 |
| C11 QueryResultTable | typed cells、receipt/query_id、summary/truncated/warnings | 精确display、复制、完整性、导出入口 | decimal/int8无精度损失、NULL≠空串 |
| C12 SafeMarkdown | sanitized structured message、validated citations | 禁activeHTML/外图/preview，links受控 | 假citation无链接；code仅text |
| C13 ChatComposer | turn/session state、allowed actions、budget | IME、多行、send/stop/end区分；防重复 | 中文输入Enter不误发；mobile软键盘 |
| C14 SecretInput | configured/ref、write/rotate能力 | 空输入不覆盖；显式replace/clear；不回读 | 无value进入URL/cache/test截图 |
| C15 Empty/ErrorState | typed reason、trace、next action | 简短说明与合法重试 | unsupported/forbidden不伪empty |
| C16 VersionStrip | project/env/catalog/policy/knowledge version、freshness | 折叠详细tuple，显示过时说明 | 用户数据空间版本变化可见 |
| C17 ObjectGraph | authorized nodes/edges、focus ID、limits | 只读局部图、搜索、列表fallback、按需展开 | hidden nodes不泄漏；keyboard/小屏可用 |
| C18 NumericChart | authorized series、units、exact display values | 柱/线/分布，tooltip、等价表 | scale/精确数值说明；不能饼图装饰 |
| C19 Wizard | step IDs、validated draft/plan version、submit capability | step error、back、unsaved提示、异步job | 改输入使旧plan失效；Secret不本地持久 |
| C20 ActionDialog | action scope、impact、reason/confirmation、server state | 低风险不无谓确认；破坏/权限扩张明确影响 | focus trap/return；不隐藏批量对象范围 |

## 统一变体和键盘

按钮default/secondary/outline/ghost/destructive；sm32/default40/mobile44px。state default/hover/pressed/focus/disabled/submitting/read-only；loading保留按钮宽度和动作文字，提交重入禁用而不是组件卸载。Disabled理由在相邻帮助文字可读，不能只有hover tooltip。输入label/helper/error必须ID关联，required显示文字或可读符号。

Dialog focus trap/Esc/返回触发点；有未提交编辑时Esc先提示而非丢数据；Dropdown箭头/Enter/Esc；Tabs按ARIA模式，复杂网络加载可manual activation；Toast只附加反馈，错误/结果在页面持久可见。ARIA live polite播报完成/断线/恢复，不逐token播报整个回答。table默认原生table，单元格交互不轻率声明grid；grid需要完整方向键/focus模型和读屏测试。

## 表格选择规范

默认checkbox只选当前页，说明“本页已选N项”；分页/筛选/排序改变后清选择并播报。跨页批量必须独立后端selection token/query seal与范围预览，前端不能把未加载记录默认为选中。三密度不改变业务字段，列隐藏偏好不是安全授权。渲染10万源行不等于一次加载10万UI行。

## Storybook与维护

每组件必须提供default、long zh/en、loading、error、forbidden、disabled/read-only以及适用conflict/empty；light/dark/compact/comfortable和reducedmotion。Storybook只publicfixture且不进默认生产部署；mock有DEMO标识。组件变更更新受影响stories/页面visual baseline，同一修复不再重复完整后端矩阵。
