# M6 发现

1. 第四次证明停在 `certutil -N`。家目录里已经有上一次留下的 `cert9.db`。对已存在的库，`--empty-password` 不生效，进程在无终端上反复打印 `Invalid password`。新建空目录时同一命令退出码是 0。修复是先删掉这个库，再用 `--empty-password` 创建，并用 `timeout 20`。当时的 node 日志有 2.3GB，只保留了头尾摘录。

2. `cp -a` 和 `tar` 在 `CapDrop ALL` 下不能保留 uid 501。`cp -R` 可以。两次失败日志在 `raw/rust-copy-eperm.log` 和 `raw/rust-tar-eperm.log`。

3. 第一次 Node 运行把 Vitest 跑在 node 环境里，`router.test.tsx` 因没有 `document` 失败。原因是命令没有带 `config/vitest.config.ts`。`raw/vitest-attempt-1.log` 保留着。后来带上配置，8 个测试通过。

4. 浏览器第一次启动失败：`libatk-1.0.so.0` 缺失。`raw/playwright-libatk.log` 保留着。随后按 M5 的同一组库，加上 `libnss3-tools`，在容器内 apt 安装。`ldd` 没有 `not found`。没有使用证书绕过。

5. NSS 信任库建在 `/home/fe01/.pki/nssdb`，昵称 `fe01ca`，信任位 `C,,`。Playwright `ignoreHTTPSErrors` 是 false。

6. 错误 CSRF 的 403 由测试断言证明，不在 `browser-receipt.json`。见 `raw/csrf-assertion.txt`。

7. G2 `apps/web/src/features/admin-overview/production-route.test.tsx` 仍要求 `getIdentityCurrent` 为 `schema_frozen`。实际值现在是 `fixture_verified`。`adminSessionReady()` 无参数时仍是 false。这份测试没有改。

8. 要求五个身份 id 仍全部是 `schema_frozen`、或它们的 `invokeOperation` 仍是 `schema_only` 的检查，已经过时。`schema_only` 仍适用于 `gateway.packMetadata.get` 和十五个 B04 operation。五个身份 id 的调用结果是 `not_connected`。

9. `ADMIN_MODULES === []` 从 M5 起就过时。本里程碑没有把它改回去。

10. 浏览器没有覆盖 reviewer、steward、auditor、business_owner，也没有覆盖客户 IdP。B04 没有真实服务。a373 的库当时在运行，没有调用。
