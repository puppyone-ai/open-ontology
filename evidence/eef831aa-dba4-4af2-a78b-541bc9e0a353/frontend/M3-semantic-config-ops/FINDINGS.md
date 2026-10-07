# M3 发现

其余管理页可以在不改 G1 文件的情况下完成注册和渲染。生产入口不会出现这些页面，直到 G1 把 `ADMIN_FEATURE_MODULES` 追加进 `ADMIN_MODULES`。

模型页和 Secret 页消费 `B07.secretWrite` 与 `B07.secretRotate`，页面上没有输入框，也不回读。`SecretInput` 在不能写时仍会显示“当前身份没有写入权限。”所以这两页继续不放它。

恢复向导保持不可提交。指标的四项状态都是尚未接入。运维页没有把进程里的 `gateway.health.get` 当成 B12 健康。

这一刀没有新增样式。生产包仍是 112 个模块，CSS 仍是 `index-GQYUKJ5S.css`。记录在案的容器没有失败阶段。类型、lint、测试和构建退出码都是 0。
