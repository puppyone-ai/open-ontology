# M6 发现

固定提交 `97909a4b7a42b39910f8277af5aa86cc899cbb99` 把五个身份 operation 记成 `fixture_verified`。这个目录状态不打开管理页。`adminSessionReady()` 只在解析后的会话拥有 `project.admin` 时为 true。

未登录、401、调用方 JSON `{"authorized": true}`、未初始化会话和 reviewer 会话都不调用模块 `load`。未初始化停在引导页，reviewer 停在没有权限。`project.admin` 会挂载总览，总览里的 B02、B03、B04、B08、B10 仍是未接入，刷新按钮禁用。

`B01.projectRoleBinding` 仍是 unbound，所以角色、身份绑定和权限预览页继续保持未接入，没有改成可用流程。`createBuild` 仍是 `schema_frozen`，`B04.builder` 仍是 unbound。a373 没有可绑定的固定服务，构建到审核分类再到 compiled pack 的路径没有接。

字体覆盖在这份固定归档上仍然失败，差的是 U+4FEE 和 U+8BA2。G2 的测试没有这两个字符，也没有改字体、清单或 SDK。

工作区里另有未提交的字体、Storybook 和 `apps/web/deploy/b01-adapter*`。它们不在这次归档里，也不进入这次提交。
