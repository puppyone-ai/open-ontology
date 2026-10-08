# M5 发现

固定提交 `1fce257f41dedd1e460cb4180d5286e256caa10d` 里，`ADMIN_MODULES` 与 `ADMIN_FEATURE_MODULES` 是同一个 41 项数组。未登录时 `GuardedAdminModule` 不调用 `load()`。`getIdentityCurrent` 停在 `schema_frozen`，`adminSessionReady()` 为 false。G2 原先要求空数组的两份测试已经落后于这个合同。

生产路由测试覆盖全部 41 条注册路径。每条都渲染「管理登录」，不渲染模块标题，不调用 `load`，不调用 `fetch`。带参数的地址不把 `route-sample` 画出来。直接 `load()` 仍只出现在注册表单测里，用来核对未接入页面；那次调用不属于生产路由。

M4 的三处还在同一归档里：连接步骤没有 secret 字段，导航没有 `:id`，`ReviewDesk` 在没有候选时不渲染 `ReviewDecisionForm`。

字体覆盖在这份固定归档上失败，而且与这两份测试无关。清单有 506 个码位，根目录扫到 504 个。多出的 U+4FEE 和 U+8BA2 在已提交的 `apps/web` 文本里不存在，G2 的两份测试里也没有。把两份测试换回 M5 原文后，失败的码位相同。字体清单、`fonts.css` 和 `font-coverage.test.ts` 都属于 G1，这次没有改。

工作区相对 HEAD 还有未提交的 `admin-gate.tsx`、auth 页、user 页和未跟踪的 `identity-context.tsx`。Linux 检查使用固定归档，没有采用这些文件。
