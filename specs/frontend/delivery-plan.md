# OO-FE-DELIVERY-001 前端实施依赖与所有权

> 2026-10-08执行方式更新：统一使用 [完整前端总任务卡](master-task.md)；最多三个固定Grok CLI会话（含复审/验收），先核对复用已有会话。下文28卡/旧分工/旧并发数保留历史或细节参考，不再作为逐卡派发指令；主进程已有台账记录和证据不删除。

## 1. 领取原则

本包在现有完整Plan内拆解28个前端交付单元，不是新28个已完成父项。当前未启动代码开发；所有卡planned。主协调在派发前填固定base/sourceowner/independentreviewer/UUID、绑定真实后端operation和资源预算。功能开发、API live、集成、发行分别统计，不能将28卡数量混入现有80冻结分母。

[任务索引](tasks/README.md) 是唯一卡目录，[任务公共协议](task-protocol.md) 是统一执行协议。优先完成用户可见纵向闭环：双入口/登录→PG接入→构建→专家审核/分类→角色Pack发布→真实Agent问答/结果→关闭；可并行页面不等所有后端就绪再画mock，但真实能力验收不得绕过依赖。

## 2. 推荐波次

| 波次 | 任务 | 并行与退出 |
|---|---|---|
| F0 基础 | FE-01→FE-04/FE-03→FE-02→FE-05 | Token/依赖/SDK/schema/route单一owner；公共契约冻结后再消费者 |
| F1 首个产品链 | FE-06/07/08/10/19并行，FE-09依jobs，FE-11依candidate，FE-12依review | 首先PG接入与实际候选审核；文件独立线避免阻塞PG |
| F2 语义与访问 | FE-13/14/15/16/17/18/20按冻结API并行 | metric/实体/码表按各自owner，发布依真实gate/Pack；不能用全部accepted绕过 |
| F3 用户面与报告 | FE-21→FE-22→FE-23；FE-24/25/26可并行 | 实际用户身份/harness/receipt前置，API缺口专门后端线 |
| F4 集成与发行 | FE-27→FE-28 | 路由串行组装，全旅程真实依赖，完整双架构在此节点 |

波次是依赖层级，不承诺日历或刻意把可独立页面阻塞。开发Grok≤20但实际按接口/资源就绪派发，review不占槽；全局Rustbuilder10，Node/browser资源由协调者按宿主实测冻结。

## 3. 所有权与共享文件

FE-01 owns manifests/lock/version/buildscripts/fonts/tokens/storybookinfra；FE-03 owns typedSDK/API/cache/bindingregistry；FE-04 owns UI/shared/stories；FE-02 owns app/layouts/router/project context；其他任务ownfeatures。页面自有module.ts引用冻结ModuleRegistration接口，不改centralrouter。FE-27集成时由FE-02完成挂接或显式移交唯一注册文件写入权，两者不并行改。

字体/theme/组件变更走原owner，不能任务间复制CSS值。新API后端producer冻结后FE-03绑定，再featureconsumer；发现更改Rust共享Schema由后端owner处理。FE-28只ownweb/deploy，既有Compose/Caddy由deploymentowner串行接纳，不把user授权文档当部署执行批准。

## 4. 开发检查与发行审批

UI开发可以在publicfixture+固定Schema上完成；真实auth/publish/runsc/customer/CI外部条件继续相应验收门槛，未到位只影响该live/发行，不阻塞无关公共页面。每卡记录UI verified/live verified/open blockers，经过fixedreview才done其卡范围。父SP完成仍按主Plan所有工作项和退出条件，不因为有页面即可签。

## 5. TODO

- [ ] [FE-01 — 工具链、字体与设计Token](tasks/FE-01.md)
- [ ] [FE-02 — 双入口Shell、路由和项目上下文](tasks/FE-02.md)
- [ ] [FE-03 — TS SDK、绑定登记与服务器状态基础](tasks/FE-03.md)
- [ ] [FE-04 — 公共组件与Storybook状态库](tasks/FE-04.md)
- [ ] [FE-05 — 双入口OIDC与一次性初始化](tasks/FE-05.md)
- [ ] [FE-06 — 数据库接入与源状态](tasks/FE-06.md)
- [ ] [FE-07 — 上传、所有格式预览与导入](tasks/FE-07.md)
- [ ] [FE-08 — 任务、checkpoint与有界事件恢复](tasks/FE-08.md)
- [ ] [FE-09 — Self-build运行与实际候选](tasks/FE-09.md)
- [ ] [FE-10 — Ontology目录、表字段与三层overlay](tasks/FE-10.md)
- [ ] [FE-11 — 专家审核队列与决定](tasks/FE-11.md)
- [ ] [FE-12 — S10列分类与批量范围预览](tasks/FE-12.md)
- [ ] [FE-13 — 指标编辑、对账与owner认证](tasks/FE-13.md)
- [ ] [FE-14 — 码表、业务日历与结账历史](tasks/FE-14.md)
- [ ] [FE-15 — 实体、术语与关系浏览器](tasks/FE-15.md)
- [ ] [FE-16 — 候选问题SQL与已验证示例](tasks/FE-16.md)
- [ ] [FE-17 — 角色、身份绑定与权限预览](tasks/FE-17.md)
- [ ] [FE-18 — 候选版本、门禁与发布恢复](tasks/FE-18.md)
- [ ] [FE-19 — 独立模型设置与只写Secret](tasks/FE-19.md)
- [ ] [FE-20 — harness、渠道、部署配置及外部客户端](tasks/FE-20.md)
- [ ] [FE-21 — 用户对话、流式与歧义澄清](tasks/FE-21.md)
- [ ] [FE-22 — 精确结果、SQL证据与安全引用](tasks/FE-22.md)
- [ ] [FE-23 — 历史、输出文件、恢复与会话关闭](tasks/FE-23.md)
- [ ] [FE-24 — 公开与客户隔离评测、版本报告](tasks/FE-24.md)
- [ ] [FE-25 — 受控审计、会话回放与终止](tasks/FE-25.md)
- [ ] [FE-26 — 健康、质量修复、备份恢复与紧急停止](tasks/FE-26.md)
- [ ] [FE-27 — 全路由组装、真实用户旅程与可访问性集成](tasks/FE-27.md)
- [ ] [FE-28 — 部署制品、性能、离线与发行准备](tasks/FE-28.md)

开工后由主进程在索引维护状态/固定source/review/evidence链接；本侧会话不会替主进程勾选或创建运行handle。来源与页面追踪见 [coverage](coverage-sources.md)。

## 主协调实施台账（2026-10-08）

| 卡 | 状态 | 开发 owner / UUID | 固定基线 | 独立 reviewer | 依赖与资源 | UI / live / 旅程 |
|---|---|---|---|---|---|---|
| FE-01 | in_progress | a351 / eef831aa-dba4-4af2-a78b-541bc9e0a351 | cb5c93cf736577637f99cda4f132f34b1fb44299 + 本次前端规范提交 | a352，交付后派发 | 无前置；单 Node4GiB1CPU/单浏览器 | 未验收 / 不适用基础业务绑定 / 未验收 |
| FE-03 / FE-04 | planned，未派发 | 待指定 | 待 FE-01 固定提交 | 待指定 | FE-01 接口与锁就绪后可并行 | 未验收 / 未验收 / 未验收 |
| FE-02 | planned，未派发 | 待指定 | 待 FE-01/04 固定提交 | 待指定 | Shell 与 centralrouter 唯一 owner | 未验收 / 未验收 / 未验收 |

FE 开发验收 0/28，实际 API 绑定与旅程尚未验收；28 卡不混入主后端冻结分母。FE-01 唯一 JS 依赖/锁 owner；前端其他卡不能改公共 Token/SDK/router。Node 构建全局 2、浏览器全局 2，每卡各最多 1；开发 Grok20、审阅不占槽、Rust10及PG每卡1继续。主线优先 S0–S10，完整范围至 SP14，前端两入口随真实契约推进，无 mock 完成替代。

## 最新执行覆盖

用户改为 [master-task.md](master-task.md) 唯一派发入口，最多 G1/G2/G3 三个固定会话（含 review），旧波次与 28 卡不再逐张派发。实际状态唯一维护于 [execution-status.md](execution-status.md)，本文件保留依赖/参考覆盖，不再作为重复动态台账。原 a351 基础会话复用为 G1；其他角色按总卡与固定接口依赖启动。
