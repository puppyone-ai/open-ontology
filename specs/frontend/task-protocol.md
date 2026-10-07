# OO-FE-TASK-001 任务卡公共协议

公共文案/术语按 [内容体系](content-system.md)。每张 [任务卡](tasks/README.md) 必须引用本文件、DS/PAGE/COMP/API/验收；卡只写本任务的页面、目录、依赖、实现步骤和判定，不重复定义公共Token/状态/权限。发现公共规范缺口交协调者更新一次，禁止在私卡中改API或设计。

## 开工

1. 主进程确认当前任务/主Plan映射、固定base SHA、真实前置交付和APIbinding；写owner+独立reviewer+资源owner。所有卡当前planned，不直接按文档当已开工。
2. 主进程唯一指定开发Grok Build CLI grok-4.7 xhigh与独立review UUID；本包不启动任务。无worktree默认，明确非重叠目录，冲突任务串行；不启用nestedsubagents。
3. 首先在Linux固定NodeLTS/pnpm锁环境跑已有最小安装/TS/build冒烟，Rust adapter变更另跑Linux/MSRV1.94对应范围。失败记录原有问题，不能删除测试掩盖。FE-01首次无前端时先冻结最小manifest/versions/lock后Linux冒烟，接着才实现UI；没有可运行基线明确“首次bootstrap”，不伪造preedit通过。
4. 新依赖仅FE-01基础线冻结；页面任务不得更改lock/package/toolchain/sharedtoken/rootRust。公共基础API已经固定才可消费者并行；后端缺失登记而不自行实现Rust服务。

## 实现与输出

按页面规范含全部公共状态、真实能力边界、响应式、键盘；交付可观察verticalslice。公开syntheticfixture，客户/密码/模型Key不进repo/publicCI/screenshots。UI写在卡指定feature目录，module.ts作为route注册输入；Shell只有FE-02串行挂接，集成时显式移交写入权。

交付本任务目录、publicfixture/story、必要测试、APIBINDING.md（group/schema/commit/operationID/path/权限/状态/live状态），以及 evidence/<run>/frontend/<FE-ID>/{REPORT,FINDINGS,HANDOFF,manifest}。报告记录实际命令/exits/原始结果、source SHA、Node/browser/OS/tool versions、截图/hash、open blockers/unsupported。缺后端与外部门槛不得记为verified integration；不制造realowner/cert/receipt。

## 验证与审阅

局部TS/lint/build+组件行为+必要Playwright/视觉/axe，不要每张页面卡重复完整双架构/Rust/安全矩阵。新缺陷才扩大验证。UI首次绑定live至少成功路径+对应权限拒绝/版本冲突/失败语义；认证/发布等必要边界不能省。合并有固定SHA独立review；作者不能自签最终验收。按source版本锁定复审，不追逐dirtyworkspace。

mainline资源沿用用户上限：Grok开发≤20（review不占），全局Rustbuilders≤10，各任务≤1实际builder（含host/install/inventory）和≤1PG；Node测试并发另由协调者按CPU/内存预算冻结，不擅用20个browser+build压垮Docker。复用已审阅script/report/recipe来源path/SHA/hash，taskUUID-private证据与资源；无privileged/hostnet/socket/共享HOME/authstore/foreigncleanup。

完成立即只提交ownedpaths，先检查暂存差异不混入主进程文件；固定提交复审，通过后协调者更新前端任务表和主Plan对应切片。若共享gitindex已有他人暂存，停止本任务commit而交协调者处理；不reset/prune/push/release/碰其他资源。任务done只代表其冻结切片，父Phase仍需全部任务+完整集成。

## 完成与计数

状态planned→ready→in_progress→in_review→verified→done；blocked附具体条件。每卡分别记录UI开发verified、live绑定verified、整旅程/发行验收，不把Storybook截图当产品已运行。28卡是此文档包实施拆分，不自动追加现有80冻结分母；必须主协调先映射计数，不能重复计API/组件/页面review。
