# M0 交接

G2 可以在这次冒烟之后写自己的 feature 目录。不改锁、Token、样式、SDK、路由、根 JS 和 Rust。

G1 继续唯一挂载 `ADMIN_MODULES`。在 B01 仍是 unbound 时，`/admin` 应保持跳到 `/admin/login`。G2 后面交出的 `module.ts` 只是注册输入。

容器 `oo-a353-node-prepare`、`oo-a353-node-smoke` 和网络 `oo-a353-net` 已删除。私有源码和 corepack 留在本机 `/tmp/oo-a353-m0`，不进 Git。
