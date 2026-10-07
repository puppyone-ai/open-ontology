# M4 发现

## 第一次浏览器失败

`oo-fe01-node-m4` 里，减少动效测试通过。字体请求测试失败：`raw/playwright-attempt-1.log`。失败点是把每条字体响应都要求成 HTTP 200。实际状态是 200 和 304。304 是同一子集地址的缓存复用。那次记录 `raw/font-requests-attempt-1.json` 里没有 `NotoSansSC-full.woff2`。

规格改为接受 200 和 304，并要求 400 子集至少有一条 200。第二次只重跑这一条测试，`raw/playwright-retry.log` 退出 0。字体、单元测试、类型检查、lint 和生产构建没有重跑。

## apt 清理提示

两次 apt 退出都是 0。第一次日志末尾有 `rm: cannot remove '/var/cache/apt/archives/partial/*.deb': Permission denied`。这是 cap-drop ALL 下 apt 自己的缓存清理，安装已经完成，ldd 没有缺库。

## 仍留在规格文件里的旧测试

`foundation.visual.spec.ts` 里其余 19 条测试仍打开 `/` 并期待标题「设计基线」。生产 `/` 现在进入登录页。这 19 条没有改，也没有跑。本次证明只来自标题含 production 的两条，第二次只跑了字体那一条。

## 资源读数

第一次容器的 `/proc/meminfo` 显示虚拟机内存总量。两次 `docker inspect` 的 HostConfig.Memory 都是 4294967296，NanoCpus 都是 1000000000。第二次 cgroup `memory.max` 是 4294967296。执行预算按 cgroup 和 HostConfig。

## 子集范围

376 个码位包含 G3 的 98 个缺口，也包含当时 G1 源码里除 U+9F98 以外的非 ASCII 字符。运行期间 G2 提交了新的管理目录。那些目录不在 `subset-sources.json` 的 roots 里，没有当作固定合同读入，也没有修改。管理页如果以后挂上路由，子集以外的字仍由完整字体回退。

## 绑定

这次没有新的 API 绑定。B01–B12 仍是 unbound。登录页写明企业身份登录尚未接入，并且不收集密码。
