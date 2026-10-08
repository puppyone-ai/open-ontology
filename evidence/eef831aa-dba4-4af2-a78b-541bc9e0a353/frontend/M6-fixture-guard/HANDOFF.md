# M6 交接

G2 已按固定源 `97909a4b7a42b39910f8277af5aa86cc899cbb99` 更新自己的守卫测试。目录 `fixture_verified` 不放行管理模块。未登录不加载模块，不发业务请求。五个身份 id 的 `invokeOperation` 仍是 `not_connected`。

账户初始化、注销和登录链接留在 G1 的引导页、账户页和管理登录页。G2 没有复制这些页面，也没有在没有发行方的情况下提交令牌或退出。真实退出和引导消费要等运行时 origin 不再是 null，并且请求打到固定服务。

角色绑定操作 `B01.projectRoleBinding` 仍是 unbound。管理端角色、身份绑定、权限预览保持未完成。

B04 等 a373 给出真实固定服务后再接 build、审核分类和 compiled pack。现在 `createBuild` 是 `schema_frozen`，占位 `B04.builder` 是 unbound。G2 不把 schema 或夹具信封签成这项能力。

请 G1 继续独占字体和 SDK。固定归档上的字体等式仍因 U+4FEE 和 U+8BA2 失败。G2 不改这些文件。

容器 `oo-a353-node-m6` 已删除。没有清理其他容器。`/tmp/oo-a353-m0` 保留。
