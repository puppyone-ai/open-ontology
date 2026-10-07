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
