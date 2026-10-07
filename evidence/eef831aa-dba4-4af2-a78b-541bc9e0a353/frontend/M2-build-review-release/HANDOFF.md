# M2 交接

G1 若要挂上这一刀，从 `apps/web/src/features/admin-overview/catalog.ts` 读取 `ADMIN_FEATURE_MODULES`，追加进 `ADMIN_MODULES`。目录现在包含 A03–A12、A17–A19、A29、A30。在 B01 仍是 unbound 时不要拿掉 `/admin` 到 `/admin/login` 的重定向。`capability` 不是授权。

Storybook 现在只收 `src/styles/**/*.stories.tsx`。feature 故事已经写在各目录的 `feature.stories.tsx`，G1 扩大 glob 之前它们不会出现在 Storybook。

`SecretInput` 的 unbound 状态仍按 M1 交接请求。这一刀没有放 Secret 输入框。

本刀新消费的 operation 都是 unbound：B04 的 builder、cards、candidates、reviews、classifications；B03.jobs；B05 的 publish、pack、gates；B06.roles。没有新的 SDK 操作。不要为这些页面补 mock。

容器 `oo-a353-node-m2` 和 `oo-a353-node-m2b` 已删除。启动 `oo-a353-node-m2b` 之前没有别的 Node 容器。当时有两个非 Node 容器在跑，没有进入，也没有清理。
