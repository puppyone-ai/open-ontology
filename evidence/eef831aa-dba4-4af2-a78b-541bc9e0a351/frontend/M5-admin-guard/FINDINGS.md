# M5 发现

## 容器

`oo-fe01-node-m5` 和 `oo-fe01-node-m5b` 的 `docker wait` 都是 0。两只容器和它们的私有网桥已按名字删除。没有 prune，没有动 `oo-a370-builder`、`oo-a370-pg` 或 `oo-a372-pg`。

apt 在退出 0 之后打印 `rm: cannot remove '/var/cache/apt/archives/partial/*.deb': Permission denied`。这不是安装失败。没有为此加权限。

## 预期会红的检查

G2 自己的两则测试仍要求 `ADMIN_MODULES` 等于 `[]`。`raw/vitest-g2-stale.log` 里失败的是这个相等断言，不是找不到「管理登录」。文件没有改。

`admin-oracle.mjs` 失败两条：`ADMIN_MODULES` 不再是空常量；`router.tsx` 不再包含原来的 `path: "/admin", element: <Navigate to="/admin/login" replace />`。运行时 `/admin` 仍由守卫替换到 `/admin/login`。

`slices-oracle.mjs` 失败两条：空数组，以及 `schema_frozen` 现在包含网关 metadata、五个控制身份 operation 和十五个 B04 operation。占位组 B01 到 B12 仍在，而且这些占位 id 的 `invokeOperation` 仍是 `not_connected`。

这些 oracle 和 G2 测试都没有改。

## 保留的边界

- 完整字体没有进这次浏览器请求。旧行为测试要求完整字体，是因为基础页渲染了 U+9F98。生产路由没有这个字。完整文件的哈希仍由字体测试锁住。
- 基础页标本上的输入框和样本按钮对比度没有在这只浏览器里重测。登录标题的实测对比是浅色 15.006、深色 14.97。Token 色值没有改。
- 减少动效的生产测试仍先打开 `/`，再确认地址是 `/login`，并且页面上没有 `[data-motion-sample]`。`toBeCloseTo(0.01, 5)` 还在。
- `index.html` 只预加载入口 JS 和 CSS。管理页分块存在，但未登录导航不会请求它们。
- 禁用的发送按钮保持禁用。问题框没有启用。

## 提交

第一次生成 `SHA256SUMS` 时宿主 PATH 为空，`shasum` 和 `git` 都是 command not found。文件被截成 0 字节，没有产生提交。容器日志没有因此重跑。

`e090131ed` 在字体生成之后给管理块增加了「数据源名称」和「没有候选」。这九个字已经在当前子集的 unicode-range 里，所以没有重跑字体生成。
