# M12 发现

1. `listBuilds` 打到身份边缘后是 HTTP 404。响应体不是已提交的 selfbuild error schema，客户端记为 `invalid_schema`。B04 成功响应还没有解析器，2xx 也不会被当成构建完成。a373 的服务交接仍写明二进制未验收。

2. `apps/web/tests/acceptance/m11-closure-oracle.mjs` 在说明仍是 M7 时失败，在说明改成五个角色后也失败。这次没有改这个 oracle。`slices-oracle.mjs` 里那条只引用 M7 的 note 依赖旧句子，源码里已经去掉。字体 oracle 仍期望子集长度 376，这次没有动字体。

3. 这次私有发行方的 config commit 是 `e24a947d3f4aec69e2c00d35e3cd132e42f4712bbdb6753a8cc583295c1536c9`。它和 M9 回执里的 `ec801f46456d62b23679c05e87f45e23cedd710bd3aedb1992d69477a2748152` 不同，因为 origin 主机名进入了 canonical commit。SDK 常量保持 M9 的值。

4. G1 自己的 M9 `browser-receipt.json` 没有把 logout 403 写进操作列表。`CONTROL_IDENTITY_NOTE` 改指 G3 封存的回执 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a352/frontend/M9-b01-roles/raw/browser-receipt.json`。这次 `client-receipt.json` 自己也有一次 logout 403。

5. 未跟踪的 source、secret、release OpenAPI 没有读进 SDK。B02 与 B07 仍是 unbound。`POST /v1/sql` 仍是 501，不是 B09。

6. https 页面会登记页 origin 并走 `callOperation`。jsdom 是 http，现有守卫测试仍看到相对路径 `/control/v1/identity/current`。这条 https 分支没有单独的浏览器截图。
