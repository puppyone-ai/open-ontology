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
