# M2 用户路由发现

## 路由选择

主任务要求未认证访问 `/` 进入 `/login`，也要求后端未接入时页面保持诚实的不可用状态。这两条同时成立的做法是：`/` 继续重定向；U03–U06 直接渲染未接入页面；U02 的对话表面留在 `ChatHomePage`，测试直接渲染它，生产路由在没有会话时不挂载它。

对话地址里的 id 不回显。测试使用 `secret-token`，页面文本里没有这段字符串。文件页没有下载链接。知识页不列出对象名。账户页没有密码框，也没有退出按钮。偏好仍只有 `oo.pref.theme`、`oo.pref.density`、`oo.pref.lang`。

## 证明边界

这次 Linux 运行没有失败尝试。类型、lint、28 个测试和构建都是一次通过。浏览器仍 withheld，本会话没有启动 Playwright，也没有和 G3 抢浏览器。

没有改锁，没有改字体，没有重跑对比度或 Storybook。

网关 blob 在 HEAD `942cbc43bba19f593192b9578f83e5f285823f85` 仍是 metadata `969e655d7458d8b679933ffb6f361f08741cabea`、error `482621857488168e85b78a11bec17855fd7649e6`、protocol `9848dcafc4c36b6512240f20fec1b09b1146c02a`。SDK 常量没有改。

`apps/web/tests/` 仍是未跟踪的外来文件，没有暂存。

## 资源

容器 `oo-fe01-node-m2` 退出码 0 后按精确名字 `docker rm`。不 prune，不碰其他容器。删除后 `oo-fe01` 计数为 0。
