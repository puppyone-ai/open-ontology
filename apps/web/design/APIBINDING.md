# APIBINDING FE-01

| 字段 | 值 |
|---|---|
| group | 无。E01 的绑定组在页面规格里写的是“全部”，本卡不调用其中任何操作。 |
| operationID | not_applicable |
| method | not_applicable |
| path | not_applicable |
| service commit | not_applicable |
| schema hash | not_applicable |
| capability | 无 |
| subject scope | 无 |
| revision | 无 |
| error mapping | 无。页面上的无权限、404、版本不兼容和网络错误是静态文案标本。 |
| event contract | 无 |
| golden fixture | `apps/web/design/copy.json`，标明不是服务端结果 |
| state | unbound |
| live | false |
| UI verified | 单元测试与生产构建已通过。浏览器截图、axe、键盘和减少动效未执行，不是 live verified |
| journey | false |
| release | false |

本卡不绑定 B01–B12。没有登录、审核、发布或会话请求。HTTP 200 与 mock 都不能当成业务接通。

## M1 公共接口

| 字段 | 值 |
|---|---|
| operationID | gateway.packMetadata.get |
| method | GET |
| path | /v1/pack/metadata |
| service commit | 97b6a9eb9caa2b6059d82bdb98b703bf5ee202d8 |
| schema hash | f77d55d45bfe86e237f167881a415f67304e9dc11251474c4155a3ac2a9353e3 |
| OpenAPI | absent |
| state | schema_frozen |
| live | false |
| fixture | packages/sdk-ts/src/fixtures/pack-metadata.example.json，形状示例，不是服务器捕获 |
| journey | false |
| release | false |

POST /v1/sql 在进程里返回 501 SQL_UNSUPPORTED，登记为 unbound，不是 B09。B01–B12 其余操作仍是 unbound。登录、审核、发布和会话都没有请求。

## M2 用户路由

U02 `/` 在没有会话时仍进入 `/login`。U03、U04、U05、U06 只渲染未接入状态。这些路由没有新的 operationID，也不发请求。B01–B12 仍是 unbound。live、journey、release 仍是 false。

## M6 测试发行方

| 字段 | 值 |
|---|---|
| operationID | startAuthorizationCodeLogin、completeAuthorizationCodeCallback、getIdentityCurrent、logoutManagementSession、consumeBootstrapToken |
| service commit | 35bd6b8157cecd89f2ae085b012b4b0137562ddb |
| contract commit | 089b00d7f70da1bc42f7e201dfa070748c651d5b |
| state | fixture_verified |
| customer integration | not_claimed |
| origin | null |
| live | false |
| journey | false |
| release | false |
| evidence | evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M6-b01-session/raw/browser-receipt.json |

证明范围只有主体 `fixture-admin`、项目 `alpha`、角色 `admin`。reviewer、steward、auditor、business_owner 和客户 IdP 没有证明。B04 仍是 schema_frozen。占位 B01.* 与 B04.* 仍是 unbound。`invokeOperation` 不发送请求。

上面这张表是 M6 当时的登记。当前服务提交见下一节。

## M7 同源适配器与拒绝访问

| 字段 | 值 |
|---|---|
| operationID | startAuthorizationCodeLogin、completeAuthorizationCodeCallback、getIdentityCurrent、logoutManagementSession、consumeBootstrapToken |
| service commit | a6f92da8a0bf872f8a30efb116c2c7b14463505b |
| contract commit | 089b00d7f70da1bc42f7e201dfa070748c651d5b |
| CONTRACT.md blob | fab0fe4e1fc52976af11faedf5653478f28a956d |
| state | fixture_verified |
| customer integration | not_claimed |
| origin | null |
| reachable | false |
| live | false |
| journey | false |
| release | false |
| proved subject | fixture-admin，项目 alpha，角色 admin |
| proved denial | fixture-reviewer，项目 alpha，角色 reviewer |
| evidence | evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M7-b01-adapter/raw/browser-receipt.json |

ID Token 的 `aud` 必须包含 `client_id`。OpenAPI 与 schema blob 仍是 089b 公布的内容。`invokeOperation` 不发送请求，这五个 id 返回 `not_connected`。steward、auditor、business_owner 和客户 IdP 没有证明。B04 仍是 `schema_frozen`，合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。占位 `B01.*` 与 `B04.*` 仍是 unbound。

## M8 部署路由

示例配置的上游仍是 null。null 不是正在运行的边缘，也不是 A20 或发布。适配器在部署填入 TLS、身份上游和静态上游之后执行固定路由：`/control` 进身份服务；没有网关上游时 `/v1` 与 `/api` 返回 `api_unbound`；OIDC 发现路径返回 `oidc_unbound`；WebSocket 返回 `ws_unbound`。这些响应不是 `index.html`。页面深链才回退到 `/index.html`。运行时 origin 仍是 null。五个身份 operation 仍是 `fixture_verified`，服务提交仍是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。

## M9 五个角色与本人账户

| 字段 | 值 |
|---|---|
| operationID | startAuthorizationCodeLogin、completeAuthorizationCodeCallback、getIdentityCurrent、logoutManagementSession、consumeBootstrapToken |
| service commit | a6f92da8a0bf872f8a30efb116c2c7b14463505b |
| contract commit | 089b00d7f70da1bc42f7e201dfa070748c651d5b |
| state | fixture_verified |
| customer integration | not_claimed |
| origin | null |
| reachable | false |
| live | false |
| journey | false |
| release | false |
| proved roles | admin、reviewer、steward、auditor、business_owner，项目都是 alpha |
| config commit | ec801f46456d62b23679c05e87f45e23cedd710bd3aedb1992d69477a2748152 |
| evidence | evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M9-b01-roles/raw/browser-receipt.json |

账户页显示签名会话里的主体、角色和能力。只有 `project.admin` 出现管理入口。reviewer、steward、auditor、business_owner 看到「没有管理能力。」，`/admin` 显示「没有权限」。这是测试发行方，不是客户 IdP。`invokeOperation` 仍不发送请求。B04 仍是 `schema_frozen`，合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。a373 交接写明实现二进制未交付、E56 未验收，所以 B04 没有进入 `fixture_verified`。占位 `B01.*` 与 `B04.*` 仍是 unbound。

## M10 显式部署配置

提交的 `b01-adapter.example.json` 不带监听地址、端口、TLS 或上游。这些字段都是 null。读取配置时，缺少 `listen.host` 会拒绝，进程不补 `127.0.0.1`，也不补已销毁的证明主机。能跑起来的配置只来自部署者填写的 `OO_B01_ADAPTER_CONFIG`。身份状态、B04 合同和运行时 origin 与 M9 相同。a373 的交接没有变成可绑定交付，所以这一轮没有接 B04 的 HTTP。

## M11 合法客户端

| 字段 | 值 |
|---|---|
| B02 | unbound。git 里没有已提交的 CONTRACT 或 OpenAPI。openApi 是 absent。ready、live、journey、release 都是 false |
| B04 | schema_frozen。合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。OpenAPI 是 `tools/selfbuild-control-api/openapi/selfbuild-control-api.openapi.yaml`。ready 是 false |
| B07 | unbound。git 里没有已提交的 CONTRACT 或 OpenAPI。openApi 是 absent。ready、live、journey、release 都是 false |
| 精准 operation | 控制身份 5 个 operationId，加上 B04 OpenAPI 的 15 个 operationId。占位 `B02.*`、`B04.*`、`B07.*` 不是合法调用字符串 |
| error | `parseControlIdentityError` 与 `parseSelfbuildError` 只接受各自已提交 error schema 的 code、160 字以内 message，以及固定常量字段 |
| CSRF | `csrfFromCurrent` 只从通过 current schema 的 `csrf_token` 取出 `X-OO-CSRF`。调用方 JSON、`authorized`、本地生成值都不算 |
| 运行时注册 | `createRuntimeRegistry` / `registerRuntime`。origin 不是 null、ready 为 true、合同提交不匹配，都会拒绝。登记结果的 origin 仍是 null |
| 调用 | `prepareCall` 在 CSRF 绑定之后仍返回 `schema_only` 或 `not_connected`。它不发送 fetch，也不把 token 放进返回值 |
| 服务 | a373 交接仍写明 E56 未验收、生产者二进制未交付、身份产品读取未验收。构建、审核、分类、发布、用户问答和全旅程都还没有真实调用 |

## M12 同源传输

| 字段 | 值 |
|---|---|
| 传输 | `registerSameOriginTransport`。origin 与 pageOrigin 必须是同一个 https origin，不能带用户信息、路径、查询或已销毁的证明主机。烘焙的运行时 origin 仍是 null |
| 调用 | `callOperation` 在登记之后发送 fetch。浏览器页用相对路径，不手写 Origin。Node 证明把 Origin 设成传输 origin 一次。POST 的 `X-OO-CSRF` 只来自 current 信封 |
| 未登记调用 | `invokeOperation` 仍不发送请求。五个身份 id 仍是 `not_connected`。`prepareCall` 仍不发送请求。`registerRuntime` 仍拒绝非 null origin |
| 五角色 | 测试发行方。admin、reviewer、steward、auditor、business_owner 都完成 login、current、logout。admin 完成 bootstrap consume。错误 CSRF 的 logout 是 403，会话仍在。回执 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M12-same-origin-client/raw/client-receipt.json` |
| 这次发行方的 config commit | `e24a947d3f4aec69e2c00d35e3cd132e42f4712bbdb6753a8cc583295c1536c9`。M9 回执里的 `ec801f46456d62b23679c05e87f45e23cedd710bd3aedb1992d69477a2748152` 保持不动 |
| B04 | `listBuilds` 实际打到边缘，HTTP 404，响应不符合已提交的 selfbuild error schema，结果是 `invalid_schema`。没有把它当成构建成功。合同仍是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`，ready 仍是 false |
| 范围 | 客户集成 not_claimed。live、journey、release 都是 false。服务提交仍是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`，合同仍是 `089b00d7f70da1bc42f7e201dfa070748c651d5b` |

## M13 selfbuild 列表

| 字段 | 值 |
|---|---|
| 路由 | `/control/v1/builds`、`/control/v1/reviews`、`/control/v1/classifications` 走 `selfbuild.upstream`。示例里这个上游是 null，请求返回 502 `selfbuild_unbound`，不再转到身份上游 |
| 登记状态 | B04 仍是 `schema_frozen`。`invokeOperation` 与 `prepareCall` 仍不发送请求。页面仍调用 `prepareCall`。`registerRuntime` 仍拒绝非 null origin |
| 真实调用 | `callOperation("listBuilds")` 在同源传输登记后发送请求。HTTP 200 只在响应对上已提交的 BuildList 时算成功。空的 `builds` 是空列表，不是完成的构建。其他 selfbuild 2xx 仍是 `invalid_schema` |
| 本次回执 | 私有空库。admin 的 `listBuilds` 是 200，`count` 0。不存在的 `getBuild` 是 403 `forbidden`。`business_owner` 的 `listBuilds` 是 403 `forbidden`。回执 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M13-b04-list/raw/b04-receipt.json` |
| 未驱动 | `createBuild`、`cancelBuild`、`getBuildCards`、`getBuildCandidates`、`listReviewItems`、`getReviewItem`、`previewReviewBatch`、`decideReviewItem`、`listClassifications`、`previewClassifications`、`confirmClassifications`、`compileBuildPack`、`getBuildPack` |
| 钉 | 合同仍是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。消费的实现提交是 `45644fca81325cf1405a38ceb79435da6e8bb090`。E56、100k、代理闭环和发布都未验收。身份提交仍是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b` |
| 范围 | 客户集成 not_claimed。live、journey、release 都是 false。ready 仍是 false。这次发行方 config commit `95bcd295cc74107110a580b50e4c2e1e8eb8b3d7cebfbf564d7b761f90f5cd42` 不替换 M9 常量 |

## M14 十五条 selfbuild 调用

| 字段 | 值 |
|---|---|
| 登记状态 | B04 仍是 `schema_frozen`。`invokeOperation` 与 `prepareCall` 仍不发送请求。页面仍调用 `prepareCall`。`registerRuntime` 仍拒绝非 null origin |
| 真实调用 | `callOperation` 对已提交 OpenAPI 里 schema 通过的 2xx 记为成功。`createBuild` 与 `cancelBuild` 的成功状态是 202。`getBuild` 的成功状态是 200。预览 `items` 必须是对象；字符串数组仍是 `invalid_schema` |
| 本次回执 | 私有空库，实现 `45644fca81325cf1405a38ceb79435da6e8bb090`。admin `listBuilds` 200 且列表为空。`listReviewItems` 200 且 items 为空。`previewReviewBatch` 200，`wrote` false，items 为空。`listClassifications` 200，一列 `public.party.id`。`createBuild` 503 `dependency.not_ready`。`previewClassifications` HTTP 200，items 是字符串，客户端记 `invalid_schema`。缺失行上的 get、cancel、cards、candidates、getReviewItem、compile、pack、decide、confirm 都是 403 `forbidden`。三次 logout 都是 200。回执 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a351/frontend/M14-b04-journey/raw/b04-journey-receipt.json` |
| 钉 | 合同仍是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。OpenAPI blob `8f94655da2d765ed0de75d002e6ff7b736c44b32`。CONTRACT blob `8b149c46f1be207bb08e558b0c9abb5347a347c8`。身份提交仍是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b` |
| 范围 | 客户集成 not_claimed。live、journey、release 都是 false。ready 仍是 false。completed_build、model_called、binaries_present 都是 false。E56、100k、代理闭环和发布未验收。这次发行方 config commit `0698fb16b7af07495b028af3494148a44057139cf200f7b390498cde6f4cce08` 不替换 M9 常量 |
