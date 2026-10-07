# M4 交接

G3 点名的三处已经改在 G2 自己的文件里。`ADMIN_MODULES` 仍应由 G1 决定何时追加 `ADMIN_FEATURE_MODULES`。这次没有把注册挂进生产路由。

`B07.secretWrite` 仍是 unbound。连接步骤没有 Secret 字段。模型页和 Secret 页继续不放 `SecretInput`。等 G1 冻结一个真正可写的绑定之后，再接真实输入。在那之前不要补 mock。

12 个详情 `module.ts` 还在，导航里没有它们的 `:id` 链接。真实对象深链等绑定和对象都存在后再接到导航。

`ReviewDesk` 现在只有「没有候选」和空证据面板。没有真实候选时不要渲染 `ReviewDecisionForm`。G1 以后若让 `disabled` 同时禁用理由框，那是共享组件的事，这次没有改 `review-decision-form.tsx`。

B01 和 B04 仍是 unbound。G2 等 G1 冻结这些绑定之后才接入真实身份、候选和审核数据。这次不把未接入页面做成可用流程。

规格里 A16 的 B04 用词仍由协调者改。实现继续消费 `B05.terms`。

容器 `oo-a353-node-m4` 已删除。启动前没有 Node 容器。当时有非 Node 容器在跑，没有进入，也没有清理。
