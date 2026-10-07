# 前端统一执行台账

唯一派发依据：[总任务卡](master-task.md)。Codex 唯一维护；旧 28 卡仅为范围参考，不再逐卡派遣或追加状态。更新：2026-10-08，Asia/Shanghai。

| 固定角色 | 固定 UUID | 实际 handle / 状态 | 已交付 source / review | 唯一所有权 / 前置 |
|---|---|---|---|---|
| G1 基础与用户端 | eef831aa-dba4-4af2-a78b-541bc9e0a351 | 60396，最近一次工具确认 live | 尚无固定交付或完整报告 | master G1 路径；当前基础 milestone，JS lock/Token/shared/SDK/router 唯一 owner |
| G2 管理端 | eef831aa-dba4-4af2-a78b-541bc9e0a353 | 预留，未启动，无 handle | 未交付 | master G2 features；等待 G1 固定公共组件/SDK/ModuleRegistration 接口后启动 |
| G3 独立复审与验收 | eef831aa-dba4-4af2-a78b-541bc9e0a352 | 预留，未启动，无 handle | 未审阅 | 只写 master 指定 review/tests/evidence；等待固定产品提交 |

前端固定会话上限 3，包含 G3；不会创建第四个前端会话。当前实际一个。非前端 Grok 开发上限 20，审阅按主线规则；全局 Rust 构建 10，每任务最多一个 Rust builder/一个 PG。Node 构建全局 2、浏览器全局 2，任务各最多一个；Node4GiB/1CPU。Node/pnpm/browser/font 精确版本等待 G1 实测锁定，不猜测填写。

基线：2069a8ae 前端规范入 Git；G1 原产品基线 cb5c93cf736577637f99cda4f132f34b1fb44299。G1 总卡切换提示已写入原 prompt；其 CLI 仍 live，不启动同 UUID 第二进程，当前基础 milestone 到交付边界后原会话承接完整 G1 范围。G2/G3 不伪造为已运行。

## TODO 与分层验收

- [ ] M0：完整总卡接管，固定三角色/范围/资源，G1 当前基础工作收集交付。
- [ ] M1：公共基座、字体/Token、组件、SDK、Shell/身份、管理基础接口固定并独立复审。
- [ ] M2：真实接入→构建→S9/S10→发布→Agent 问答→关闭纵切。
- [ ] M3：全部 50 页面、B01–B12 与 C01–C20 完整覆盖。
- [ ] M4：A01–A20、真实十万行 Web 旅程、部署及适用发行门槛验收。

UI 开发、实际 API 绑定、完整旅程、发行均未验收。28 卡参考开发 0/28；不并入主线后端 81 子项分母。未实现身份/发布/harness/释放只记录依赖，不用 mock/本地鉴权/伪 receipt 替代完成。所有固定 source、独立 review、证据入口和原始退出在交付后写入此台账。主 Spec/完整 Plan 包括 SP14 的标准不变。

最新统一观测：G1 handle60396 已退出0，但尚缺完整最终报告；原 UUID 恢复时先收集已启动的 Linux/浏览器 proof，不复制资源。固定三角色不变。主线新增独立任务不占前端三角色，会话复用约束仅适用于前端。统一等待已改18分钟。

最新18分钟统一观测：G1 handle44149 terminal0，尚缺完整报告；Node verify2 已启动但未核验最终结果。恢复原a351收集既有proof，不重复字体/锁构建。G2/G3仍预留未启动，验收计数不变。

G1 handle37146 terminal0；unit6/types/lint/contrast/生产build/Storybook已报告通过但尚未完整核验，browser库安装受APT降权限制失败。原a351收集browser-only重试实际退出，不重复绿测试。允许固定部分milestone提交并明确browser WITHHELD，未通过UI验收；shared组件/SDK/ModuleRegistration固定handoff后G2开始。G3只用预留a352。

固定基础切片：2c032402ee3b58801f30dbbcefddee8733e383b1；三报告已读，94 SHA条目一致。Node24.21.0/pnpm12.10.1/Chromium153.0.8010.12 build1243，固定Nodeimage481d53bf…，单位6/TS/lint/contrast/build/Storybook0；浏览器未打开、截图0，FE-01未完成。G3 a352正式派发固定源独立浏览器/设计复审，G1原a351继续总卡公共组件/SDK/ModuleRegistration/Shell；G2a353尚等待公共接口固定。前端最多3固定会话，当前将运行G1/G3两条，开发/live/旅程/发行均未验收。


2026-10-08：G1 M1 e61c0fd98b6c8b09c9c8e2db3a441aafaf952909、M2 a08b35769 已固定交付，三报告已读（22/28单位测试、类型/lint/build0；B01–B12仍unbound、live/journey/release false）。G3基础2c032402的APPROVE_LIMITED报告已读，实际20浏览器测试/19截图；哈希待协调核验，不勾选FE-01或M1。G3要求G1先补三级Token并修减少动效测试，原a351续接，G2a353因此暂未启动。G3a352仍唯一独立前端复审会话。

运行句柄登记（一次18分钟等待后统一观测）：a358=41077、a362=54243、a364=85335（既有live保留）；a351=18247（前端G1原会话）；a360=24132、a363=86745、a361=17878（原复审会话收集/裁决）；a365=86391（新迁移修复）；a366=9979（新发布复审）。前端G2a353尚未启动，G3a352上一CLI已退出。无需同UUID另启进程。

2026-10-08 本轮18分钟一次观测完成：a358/a364/a351/a360/a363/a361退出0；a362=54243、a365=86391、a366=9979仍live，未反复观测。TLS/规则/建议独立报告现已交付，尚待全报告与SHA核验，不更新已验收数。a361明确WITHHOLD E51，已新派a368=76207修复真实共同静止切点，不减验收要求；a367=17572固定6b3a92c2审核台账独立复审。G1三级Token修复866cd4e2b完整三报告已读，故固定G2a353=2137正式启动管理总卡，G3原a352=10589复审固定接口/用户路由/Token。G1当前退出无重复CLI，前端固定会话总数仍3。下一观测仍一次18分钟，实际已交付与验收计数分开。

基础G3a352 184 SHA条目、G1Token M3 20 SHA条目核验0坏/0缺；三报告完整已读。基础固定切片独立APPROVE_LIMITED成立，M1/M2用户及新版Token仍待G3固定复审；M0–M4/A01–20/FE-01不勾选。

2026-10-08 一次18分钟统一观测：a362/a365/a366/a352退出0；a368=76207/a367=17572/G2a353=2137仍live。Builder be9acbc23完整三报告/前置FREEZE已读：新pack-combined经一次核心reseal在同两角色Pack保留party/account描述，9+12与6+21测试/Clippy0，存储100k真实模型证据未变，无新模型/PG；E47未验收，独立原完整范围复审a369=16483已派。a365缺最终包，原UUID=17316收集既有真实proof，不复制资源。G3 M1-M3完整报告已读，固定路由 limitedapproved，但作者motiontest打不到实际节点/98普通中文字落入大fallback，G1原a351=44027聚焦修复不改G2。a366发布复审完整包已存在待全读/哈希，E53不先勾选。下一UUIDa370尚未使用。计数73/86、历史98、首审88、返工14、作者94/集成0/4发行0/6保持；无父项完成。
