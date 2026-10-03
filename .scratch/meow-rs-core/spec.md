# Spec: meow-rs 平级第二内核

Status: ready-for-agent

## 修订日志

- **v2(2026-10-03)**:工单 04 实证,服务模式 meow 被外部服务阻塞——钉版的 Clash
  Verge Service 硬编码 mihomo 的 IPC 启动参数与就绪判定,meow 无法被其托管,非本
  fork 代码可解。app 侧链路(服务模式 API 传输按内核分流、能力开关、安装包向服务
  目录投递 meow 与 wintun)已就绪。据此:US 8 收窄为「sidecar 完整可用,服务模式待
  外部阻塞解除后翻能力开关」;能力模型清单补第六项 `service_hosting`(meow 为
  false,服务安装入口灰显);US 11 补充兼容性背书与人工验收口径。正文同步修正
  Solution 与「服务模式」决策段中与该事实冲突的表述。
- v1(2026-10-02):初始版本,ready-for-agent。

领域词汇见根目录 `GLOSSARY.md`(「内核」「内核能力」为本 spec 新增);测速通道决策见 `docs/adr/0001-download-speedtest-via-global-listener.md`;应用自更新禁用决策见 `docs/adr/0002-fork-builds-no-upstream-updater.md`。

## Problem Statement

维护者希望用内存占用更低、二进制更小的代理内核。meow-rs(Rust 重写的 mihomo 兼容内核,官方 Windows x64 二进制 7.8MB,内存约为 mihomo 的 1/3,周更发版)与 mihomo 的 REST/WS/配置 schema 高度兼容,但上游 clash-verge-rev 拒绝接入([issue #7571](https://github.com/clash-verge-rev/clash-verge-rev/issues/7571),Closed as not planned,无讨论)。当前 fork 的内核抽象实际只支持 mihomo 系(`verge-mihomo` / `verge-mihomo-alpha`),无法切换到 meow-rs。

## Solution

meow-rs 作为**平级第二内核**接入:

- 设置页内核选择新增 meow-rs 项,与 mihomo 平级一键切换;默认内核保持 mihomo;
- meow-rs 官方 Windows x64 zip(含 wintun.dll)按钉死的版本号在 CI 构建时下载,作为第二 sidecar 打包进安装包,安装包自包含、可离线;
- sidecar 模式完整支持 meow.exe(含 TUN);服务模式托管 meow 被外部 Clash Verge Service 阻塞,待其解除后翻能力开关启用(修订日志 v2);
- 订阅/覆写配置管线单份产出,按当前内核打补丁后喂给任一内核;
- meow 刻意不实现的接口(重启、GEO 更新、内核升级)由 fork 侧补齐;补不了的(UDP 连接跟踪、规则命中计数)在 meow 激活时 UI 灰显标注;
- fork 自有的节点下载测速在 meow 下改用 `IN-PORT` 规则引流(见 Implementation Decisions),行为与 mihomo 下一致;
- 内核升级 UI 复用现有 staging/回滚机制,meow 激活时更新源指向 meow-rs 官方 releases。

## User Stories

Actors:维护者(唯一用户,不写代码)。

1. 作为维护者,我想在设置页把内核一键切换为 meow-rs,以便用更低的内存占用日常代理。
2. 作为维护者,我想随时一键切回 mihomo,以便 meow-rs 出问题时无损退回。
3. 作为维护者,我想首次安装 fork 后默认仍是 mihomo,以便升级 fork 不改变我当前的内核选择。
4. 作为维护者,我想切换内核时自动校验配置并重启代理,以便切换后立刻生效且无需手动重启。
5. 作为维护者,我想切换失败(配置校验不过、二进制缺失)时自动回滚并看到明确提示,以便不至于处于断网状态。
6. 作为维护者,我想我的订阅、Merge、Script 覆写对两个内核同样生效,以便切换内核不需要改配置习惯。
7. 作为维护者,我想在 meow 下修改端口后自动重启内核生效,以便不必知道「meow 不支持监听热替换」这种细节。
8. 作为维护者,我想在 meow 下正常使用系统代理与 TUN(sidecar 模式完整可用;服务模式暂被外部 Clash Verge Service 阻塞,待其解除后翻 `service_hosting` 能力开关即可启用),以便与 mihomo 体验无差。
9. 作为维护者,我想在 meow 下继续使用下载测速(整组/单项/排序/自定义 URL/测速不改分组选择),以便换内核不失去 fork 的标志性功能。
10. 作为维护者,我想测速期间应用崩溃或退出后,下次启动能按恢复日志把 GLOBAL 选择恢复到测速前,以便不留测速残留(与 mihomo 下行为一致)。
11. 作为维护者,我想在 meow 下延迟测试、节点切换、代理提供者健康检查等日常操作照常工作,以便代理页无感。meow 兼容性以官方兼容性文档背书(见 Further Notes),测试包人工验收补确认。
12. 作为维护者,我想在 meow 下手动检查并更新 meow 内核版本(staging+回滚保护),以便追新不依赖 fork 发版。
13. 作为维护者,我想在 meow 下手动更新 GEO 数据库,以便规则集保持新鲜。
14. 作为维护者,我想 meow 不支持的功能(UDP 连接列表、规则命中计数)在界面上灰显并标注「meow 不支持」,以便明白是内核能力差异而非故障。
15. 作为维护者,我想连接、日志、流量、内存页在 meow 下正常展示,以便日常可观测性不缩水(UDP 会话缺失除外,按第 14 条处理)。
16. 作为维护者,我想两个内核的分组选择持久化相互隔离,以便切来切去不互相污染。
17. 作为维护者,我想设置页与关于页显示当前 meow 内核版本号,以便确认升级是否生效。
18. 作为维护者,我想安装包自包含 meow 二进制,以便离线环境也能安装使用。

## Implementation Decisions

**形态与切换**:内核标识枚举新增 meow 取值(命名沿用 verge 侧car 前缀惯例);切换复用现有切换命令的「校验(`-t`)→ 更新设置 → 重启内核」流程,失败自动回滚并提示。默认内核 mihomo,已有用户的设置不变。

**二进制分发**:CI 构建时从 meow-rs 官方 GitHub release 下载版本号钉在仓库配置里的 `x86_64-pc-windows-msvc` zip,解出 `meow.exe` + `wintun.dll` 作为第二 sidecar(externalBin)随 NSIS 安装包分发。二进制不进 git;版本号随 fork 发版由 agent 评估 bump。WSL 开发环境可下载 Linux 版 meow 用于 spike 与自测。

**服务模式**:钉版的 Clash Verge Service 硬编码 mihomo 的 IPC 启动参数与就绪判定,无法托管 meow(工单 04 实证,v2);app 侧已按内核分流服务模式的 API 传输与能力开关,待外部服务解除阻塞后翻 `service_hosting` 即可启用。meow 的 Windows TUN 走 zip 自带的 wintun.dll;sidecar 模式已完整可用。

**配置管线**:订阅/覆写产出单份 mihomo 兼容 YAML;在管线末端新增「按当前内核打补丁」步骤(meow 激活时:剔除/替换 meow 不支持的键、TUN 场景强制 `dns.enhanced-mode: fake-ip`、测速通道按内核切换注入方式)。meow 的 `strict` 模式不开,未认识的 mihomo 专有键走其默认 warn-and-skip。改端口等监听类变更在 meow 下由适配层转为内核重启(已有 fork 侧重启命令可复用)。

**API 适配**:前端外部插件 `tauri-plugin-mihomo-api` 原样用于两内核兼容的接口(proxies/延迟/连接/日志/流量/providers/规则等);重启本就由 fork 侧进程管理完成,不依赖内核 `/restart`;GEO 更新与内核升级包装为 fork 侧命令,按当前内核分流。前端新增一层薄薄的「内核能力」消费点:UI 灰显与接口分流都从能力模型取事实,不在组件里散落 `if 内核 === ...`。能力清单六项:UDP 连接跟踪、规则命中计数、监听器热替换、服务托管(`service_hosting`,v2)、GEO 更新通道、内核升级通道。

**测速适配**:mihomo 下维持现状(专用 listener 绑 `proxy: GLOBAL`,见 ADR-0001);meow 下该绑定不存在,改为注入栈顶规则 `IN-PORT,<测速专用端口>,GLOBAL` 把专用端口流量引向 GLOBAL,配合现有 GLOBAL 读写接缝与恢复日志。此前置条件是 spike 验证(见 Further Notes);spike 失败则回退为「meow 下不提供测速按钮(灰显标注)」,不影响其余功能。

**内核升级**:复用现有内核升级通道(staging 目录 + 失败回滚),meow 激活时更新源指向 `meow-rs/meow-rs` 官方 releases 的 Windows zip。GEO 更新同路:fork 侧下载与 mihomo 相同格式的 geoip/geosite 文件替换后重启内核。

**不采用 meow 原生扩展**:内置 web 面板(`/ui`)、`/api/subscriptions` 订阅管理、`/api/proxy-groups` 组 CRUD、`/metrics` 均不接入——verge 是唯一的面板与订阅事实源,避免双头管理。

**仓库约定落地**:合入 `dev` 时在 `AGENTS.md` 功能清单登记;双内核架构与测速 IN-PORT 适配两项决策补 ADR;实现走 `feat/meow-rs-core` 分支。

## Testing Decisions

只测外部行为:配置补丁函数给 YAML 断言产出;能力模型给枚举断言判定;不 mock 内核进程本身。沿用既有惯例(Rust 端模块内 `#[cfg(test)]`,前端 vitest)。

接缝(已与维护者确认,尽量高、尽量少):

1. **配置补丁纯函数**(新):输入运行时配置 + 当前内核,输出补丁后 YAML。覆盖测速通道注入的内核分叉、TUN/fake-ip 强制、meow 不支持键处理。挂接点与现有测速监听注入同位。
2. **内核能力模型**(新):中心化描述当前内核支持什么;前端适配与 UI 灰显唯一事实源。能力判定逻辑单测。
3. **测速 GLOBAL 读写接缝**(既有,复用):测速功能已有的「GLOBAL 读写最小接缝 + 注入假实现」与恢复日志纯函数不动;meow 适配只改配置注入侧,既有测试资产继续生效。
4. **内核切换与校验**(既有流程扩展):内核枚举校验数组扩展、`-t` 配置校验与 `-d`/`-f` 启动参数复用现有流程,扩展点补单测。
5. **实证 spike**(不在 CI):见 Further Notes,结果记入本目录工单。

CI 构建接线(sidecar 下载、service 托管参数)不做单测,以 CI 出包 + Windows 实测(测试包工作流)验收。每步交付的验证门:`pnpm typecheck && pnpm test`,以及 `src-tauri` 下 `cargo check`。

## Out of Scope

- meow 原生订阅管理、内置面板、metrics、组 CRUD 的任何接入;
- 向 meow-rs 上游提 PR 补 listener `proxy:` 绑定等缺口(交付时间不可控,不作为主路径);
- macOS/Linux 平台的分发与长期支持(fork 仅发 Windows x64;Linux 二进制仅用于开发自测);
- meow 缺失能力的自研补齐(UDP 连接跟踪、规则命中计数)——保持灰显标注;
- 应用内自更新(维持 ADR-0002 禁用;内核升级是另一条通道,不受影响);
- mihomo Alpha 与 meow 的组合矩阵测试(Alpha 内核与 meow 互斥选择,不做交叉回归)。

## Further Notes

**spike 清单**(实现前必须验证,WSL 跑 Linux 版 meow,结果记工单):

1. `IN-PORT,<专用测速端口>,GLOBAL` 规则能否把该端口流量引向 GLOBAL 组(mixed listener + rule 模式);
2. GLOBAL 组可被 `PUT /proxies/GLOBAL` 切换,且选择跨进程重启持久化(meow 的 selector 持久化声称镜像 mihomo cache.db 行为,测速崩溃恢复依赖它);
3. WS `/traffic`、`/logs`、`/connections`、`/memory` 与 verge 前端现有客户端完全兼容(meow 支持 `?token=` 查询参数鉴权,与 mihomo 一致);
4. `meow -t -d <dir> -f <config>` 校验与 sidecar/service 启动参数兼容性;
5. mihomo 专有键喂给 meow(非 strict)的 warn-skip 实际行为,确认配置管线单份产出可行。

**兼容性事实源**:meow 仓库 `docs/mihomo-api-compatibility.md`(以 commit `cbd11db` 为参照基线)明确列出:兼容面(version/proxies/group/延迟/connections/logs/memory/configs 读写/规则/providers/缓存清理);刻意缺口(`/restart`、`/upgrade*`、`/configs/geo`、`/storage/*`、`/rules/disable`、UDP 会话、规则命中计数、provider subscriptionInfo);扩展(`/api/*`、`/metrics`、`/dns/results`、`/listeners`)。meow 发版节奏约每周,每次 bump 内核版本时对照该文档复核差异面。

**已知体验差异**(meow 激活时,按 User Story 14 处理):连接页无 UDP 会话;规则页无命中计数;mihomo 专有协议(tuic、mieru 等)节点不可用;meow 独有协议(AnyTLS、Snell v3–v6)在 mihomo 下不可用。
