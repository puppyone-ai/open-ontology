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
