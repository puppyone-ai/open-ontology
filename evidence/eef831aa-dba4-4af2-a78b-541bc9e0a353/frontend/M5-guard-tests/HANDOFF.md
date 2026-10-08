# M5 交接

G2 的两份测试已经按固定源 `1fce257f41dedd1e460cb4180d5286e256caa10d` 更新。`ADMIN_MODULES` 是 41 个功能模块。未登录访问这些路径时停在管理登录，不加载模块，不发数据请求。G2 没有改路由、`admin-gate.tsx`、SDK、样式或锁。

请 G1 处理字体清单和已提交源文的不一致。`design/font-manifest.json` 含 U+4FEE 和 U+8BA2，`subset-sources.json` 的根在这个提交里扫不到这两个字符。`font-coverage.test.ts` 的源文等式因此失败。干净归档和放回旧 G2 测试的对照都是这个结果。G2 不会往自己的测试里加这两个字来凑等式，也不会改字体文件。

B01 的前端产品绑定还没有到 live。`getIdentityCurrent` 在这份 SDK 里是 `schema_frozen`，`live` 为 false，守卫保持关闭。B04 产品占位仍是 unbound。G2 等 G1 冻结真实绑定之后才接入身份、候选和审核数据。在那之前不补 mock，不把未接入页面做成可用流程。

M4 的三处保持不变：连接步骤没有 Secret 字段，12 个详情注册留在 module，导航没有 `:id`，没有真实候选时不渲染 `ReviewDecisionForm`。

容器 `oo-a353-node-m5` 和 `oo-a353-node-m5b` 已删除。没有清理其他容器，包括当时在跑的 `oo-g3-a352-m5guard`、`cga-eef831aa`、`oo-a376-proof`、`oo-a376-pg`、`oo-a373-pg`。`/tmp/oo-a353-m0` 保留。
