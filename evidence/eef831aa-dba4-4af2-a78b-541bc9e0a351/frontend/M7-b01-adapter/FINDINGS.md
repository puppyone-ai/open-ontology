# M7 发现

1. 第一次闭环的二进制是 `35bd6b8157cecd89f2ae085b012b4b0137562ddb`，而 `tools/control-identity` 的 HEAD 已经是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。那次 Playwright 1 通过，归档在 `raw/service-35bd/`。当前目录、`ready.json` 和浏览器收据钉的是重新启动后的 a6f。两次都是真实请求。SDK 只保留 a6f。

2. 适配器测试第一次失败，因为 openssl 在临时目录外找 `ca.srl`。日志在 `raw/adapter-test-attempt-1.log`，`raw/launch-verify-attempt-1.exit` 是 1。修复是把 openssl 的工作目录设到临时目录，并用 `-CAcreateserial`。第二次 4 通过。

3. 通过的适配器测试打印了 Node DEP0123：对 IP 设置 TLS ServerName。断言仍然通过。`rejectUnauthorized` 保持 true。

4. 错误 CSRF 的 logout 由测试断言为 403。`page.request` 的响应没有进入 `raw/browser-receipt.json`，收据里没有 403。

5. 服务钉从 35bd 改成 a6f 之后，三次 `tsc` 退出码是 0。紧接着的 Vitest 跑在只读挂载上，写入 `apps/web/node_modules/.vite-temp` 时得到 EROFS。`raw/vitest-after-pin.exit` 是 1，日志在 `raw/vitest-after-pin.log`。这不是产品断言失败。完整的 30 文件、63 测试是在改钉之前通过的。那些测试比较导入的 `CONTROL_IDENTITY_SERVICE_COMMIT`，没有把 35bd 写死。

6. `apps/web/tests/acceptance/m4-font-oracle.mjs` 第 101 行仍要求 `subsetCodepoints.length === 376`。当前清单是 504。本会话没有改这个评审脚本。

7. `apps/web/tests/acceptance/slices-oracle.mjs` 第 424 行仍要求 `serviceCommit === 35bd6b8157cecd89f2ae085b012b4b0137562ddb`。当前常量是 a6f。本会话没有改这个评审脚本。缺 schema 哈希文件和 react-router 的 fetch 符号仍按先前结论算评审夹具问题，产品没有拆掉 fetch，也没有伪造后端。

8. 这一轮浏览器只有默认视口的两张浅色截图。没有 1440、1280、768、390，没有深色、键盘、减少动效或 axe。

9. 拒绝访问的主体是测试发行方里的 `fixture-reviewer`，角色 reviewer，项目 alpha。steward、auditor、business_owner 没有证明。这不是客户 IdP。`customer_integration` 是 `not_claimed`。

10. B04 仍是 `schema_frozen`，合同 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。没有绑定 a373 的运行中服务。占位 `B01.*` 和 `B04.*` 仍是 unbound。五个身份 id 的 `invokeOperation` 返回 `not_connected`，不调用 fetch。

11. `apps/web/.storybook/preview.tsx` 有别人留下的未提交改动。本会话没有暂存，也没有还原。G2 的 19 个管理目录没有被本会话修改。

12. 字体相等断言没有改。没有为了凑码位加入中文。U+4FEE 和 U+8BA2 是从更早的 d416 子集里留下来的，固定根 `0c3caa184` 加上当前 G1 文案走查时不包含它们，所以这一轮的三个子集里没有这两个码位。U+9F98 仍只留给完整字体。
