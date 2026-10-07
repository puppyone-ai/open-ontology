# M2 发现

构建、审核、分类和发布可以在不改 G1 文件的情况下完成注册和渲染。生产入口不会出现这些页面，直到 G1 把 `ADMIN_FEATURE_MODULES` 追加进 `ADMIN_MODULES`。

S0 到 S10 保持 `not_run`。审核表单保持禁用，这一页没有保存决定。15 个发布门禁保持 `not_run`，没有 `pass`。分类页没有把 unknown 写成 0。

这一刀没有新增样式。生产 CSS 是 `index-GQYUKJ5S.css`，raw 20.07 kB，与编辑前的冒烟包一致。M1 那次归档的 CSS 哈希不同，是因为当时 HEAD 上的样式表不同。

记录在案的容器没有失败阶段。类型、lint、测试和构建退出码都是 0。
