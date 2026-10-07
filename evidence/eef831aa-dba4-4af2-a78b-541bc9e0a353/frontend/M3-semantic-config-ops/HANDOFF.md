# M3 交接

G1 若要挂上 G2 的全部注册，从 `apps/web/src/features/admin-overview/catalog.ts` 读取 `ADMIN_FEATURE_MODULES`，追加进 `ADMIN_MODULES`。目录现在包含 A03–A43。在 B01 仍是 unbound 时不要拿掉 `/admin` 到 `/admin/login` 的重定向。`capability` 不是授权。

Storybook 现在只收 `src/styles/**/*.stories.tsx`。feature 故事已经写在各目录的 `feature.stories.tsx`，G1 扩大 glob 之前它们不会出现在 Storybook。

请给 `SecretInput` 一个 unbound 状态，不要把“没有身份”说成“没有权限”。在那个状态固定之前，模型页和 Secret 页不会放输入框。

本刀新消费的 operation 都是 unbound：B01.projectRoleBinding；B04.candidates；B05 的 terms、codeTables、calendar、metric、entity、examples、pack；B06 的 roles、bindings、permissionPreview、oauth、clientToken、revoke；B07 的 secretWrite、secretRotate；B08 的 session、close；B09.query；B10 的 suites、run、result、compare、privateHoldout；B11 的 models、harness、channels、deployment、git、config、plugin；B12 的 health、audit、replay、emergency、backup、upgrade、recovery。没有新的 SDK 操作。不要为这些页面补 mock。`gateway.health.get` 仍不是 B12。

容器 `oo-a353-node-m3` 已删除。启动前没有别的 Node 容器。当时有两个非 Node 容器在跑，没有进入，也没有清理。
