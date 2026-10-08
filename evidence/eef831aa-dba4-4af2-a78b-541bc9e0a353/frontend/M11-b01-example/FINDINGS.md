# M11 发现

固定提交 `b12b5b41924754d1683c92d5a34ec1f49767839f` 的 `callOperation` 在同源 https 传输登记之后发送 fetch。`fetchImpl` 只用于这次测试。产品页面不传入它。`logoutManagementSession` 与 `/admin/bootstrap` 的 `consumeBootstrapToken` 仍是页面上的调用。`prepareCall` 的 `not_connected` 没有替换这两条。

未初始化会话从 `/admin` 进入「初始化」。提交引导令牌后，页面显示「已经初始化。客户集成 not_claimed。」。总览显示 `fixture-admin · alpha · admin`，「刷新」禁用。角色页显示主体和「初始化状态 已初始化」，预检行是「B06.roles 预检 not_precise」，「应用」禁用。角色页没有 `B01.projectRoleBinding` 预检行。退出后标题是「管理登录」，主体和总览标题消失。相对路径请求依次为：

`GET /control/v1/identity/current`

`POST /control/v1/auth/bootstrap/consume`

`GET /control/v1/identity/current`

`POST /control/v1/auth/logout`

`GET /control/v1/identity/current`

同源 https 例程里，`B01.projectRoleBinding` 与 `registerSource` 在任何请求之前返回 `not_precise`，`fetched` 为 false。随后的绝对 URL 依次是 current、consume、current、logout、current、`GET https://admin.example/control/v1/builds`。每条请求的 Origin 是 `https://admin.example`。401 current 的结果是 `http_error`，`errorCode` 是 `unauthenticated`。`listBuilds` 的 404 纯文本是 `invalid_schema`，`ok` 为 false。`live`、`journey`、`release` 为 false。

类型检查、eslint 和 vitest 是 0：34 个文件，79 个测试。G2 没有写字体、SDK、路由或 eslint 配置。

容器 `oo-a353-node-m11` 退出 0，随后删除。没有清理其他容器。
