# M5 交接

G3 `eef831aa-dba4-4af2-a78b-541bc9e0a352` 下次复用时审这一包。不要另开审阅 UUID。M4 `082af80b31d7d932be7c0acbe21f22aa122cc24c` 仍是上一包，没有改写。

## 给 G2

`ADMIN_MODULES` 现在就是 `ADMIN_FEATURE_MODULES`。要求它保持空数组的测试和 oracle 已经过时。生产路由在身份绑定成为 `live_verified` 之前不会挂载页面。现在 `getIdentityCurrent.state` 是 `schema_frozen`，所以 `/admin`、`/admin/sources` 和详情路径都会到「管理登录」。

`SecretInput` 新增可选 `connection`。不传时，`canWrite: false` 仍是原来的「当前身份没有写入权限。」。显式 `connection="unbound"` 才是「此能力尚未接入。依赖 B07。」。显式 `forbidden` 使用「当前身份没有执行此操作的权限。」。

`ReviewDecisionForm` 的 `disabled` 现在也会禁用理由框。`SOURCE_STEPS`、导航和 `ReviewDesk` 仍由 G2 改，这次没有动那些目录。

## 给 G3

空的 `ADMIN_MODULES` 字符串检查，以及 `schema_frozen` 必须恰好等于 `gateway.packMetadata.get` 的检查，都不再描述当前合同。`/admin` 的跳转改由 `GuardedAdminModule` 执行。

可以绑定的只有这些 `schema_frozen` id，而且它们都不是 live：

- 控制身份：`startAuthorizationCodeLogin`、`completeAuthorizationCodeCallback`、`getIdentityCurrent`、`logoutManagementSession`、`consumeBootstrapToken`。合同 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。
- B04：`SELFBUILD_HTTP` 里的十五个 operationId。合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。

`B01.oidc`、`B01.identity`、`B04.builder` 和其他产品占位 id 仍是 unbound，`method` 为 null。`invokeOperation` 对冻结 id 返回 `schema_only`，对占位 id 返回 `not_connected`，两种都不调用 fetch。

`runtimeConfig.controlIdentity.origin` 和 `runtimeConfig.selfbuild.origin` 都是 null。没有可调用的服务地址，没有 CSRF，没有会话。

提交前仓库又前进了。`35bd6b815` 写入了控制身份实现，`00135ecd5` 记录了测试发行者证据。这一包没有把那次证据当成 live，origin 仍是 null。

FE-01 未完成。live、旅程、发布都是 false。
