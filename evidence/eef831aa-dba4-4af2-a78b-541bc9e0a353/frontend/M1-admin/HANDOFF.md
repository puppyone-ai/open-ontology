# M1 交接

G1 若要挂上这一刀，从 `apps/web/src/features/admin-overview/catalog.ts` 读取 `ADMIN_FEATURE_MODULES`，追加进 `ADMIN_MODULES`。在 B01 仍是 unbound 时不要拿掉 `/admin` 到 `/admin/login` 的重定向。`capability` 不是授权。

Storybook 现在只收 `src/styles/**/*.stories.tsx`。feature 故事已经写在各目录的 `feature.stories.tsx`，G1 扩大 glob 之前它们不会出现在 Storybook。

请给 `SecretInput` 一个 unbound 状态，不要把“没有身份”说成“没有权限”。在那个状态固定之前，模型页和 Secret 页不会放输入框。

本刀消费的 operation 都是 unbound：B02 的 connectorMatrix、sourceConfig、inspect、catalog、dba、plan、apply；B03 的 jobs、checkpoints、events、cancel、retry；B04.builder；B07 的 secretWrite、objectPart、objectComplete、objectAbort、download、objectDelete；B08.session；B10.suites。没有新的 SDK 操作。不要为这些页面补 mock。

容器 `oo-a353-node-m1`、`oo-a353-node-m1b`、`oo-a353-node-m1c` 已删除。
