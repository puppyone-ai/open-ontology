# M8 发现

1. M7 改钉之后的 Vitest 跑在只读挂载上，写入 `apps/web/node_modules/.vite-temp` 时得到 EROFS。`M7-b01-adapter/raw/vitest-after-pin.exit` 仍是 1。这次在私有可写归档上重跑，30 个文件、63 个测试通过，包含 G2 的 `production-route.test.tsx` 和 `modules.test.tsx`。没有把失败改成跳过。

2. 主机 Node 26 把 TLS ServerName 设成 `127.0.0.1` 时直接抛 `ERR_INVALID_ARG_VALUE`。说明在 `raw/host-node26-sni.txt`。适配器对 IP 上游不再发送 SNI，域名仍发送 SNI。Linux Node v24.21.0 上的 5 个适配器测试在这个修改之后通过。证书校验没有关掉。

3. 示例配置的上游是 null。它只声明形状和固定路由，不能单独把边缘跑起来。网关和 WebSocket 目前没有可绑定的服务，对应路径返回 `api_unbound` 或 `ws_unbound`，不是已接通。

4. 这一轮没有新的浏览器截图。M7 的闭环截图和默认视口限制仍然有效。steward、auditor、business_owner 和客户 IdP 没有新证明。

5. `apps/web/.storybook/preview.tsx` 仍有别人留下的未提交改动。这次没有暂存，也没有还原。G2 目录没有改。

6. B04 仍是 `schema_frozen`。占位 `B01.*` 和 `B04.*` 仍是 unbound。`invokeOperation` 对五个身份 id 仍返回 `not_connected`。A20、整段旅程和发布都没有完成。
