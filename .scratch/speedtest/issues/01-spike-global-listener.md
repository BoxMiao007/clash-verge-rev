# 01: Spike:验证 GLOBAL 专用测速通道

**What to build:** 不含产品 UI 的机制验证(对应 spec 与 ADR-0001):临时在 mihomo 配置中注入一个仅绑定 127.0.0.1 的 mixed listener,`proxy` 字段绑定为 GLOBAL;通过 REST API 把 GLOBAL 切到某个被测节点,从该端口发起 HTTP 限时下载,验证三件事:① 下载流量出口确为被测节点(可对照切换前后的出口 IP);② 规则模式下用户其他流量不受影响;③ PUT GLOBAL 恢复原值后一切如初。验证结论以 Comments 形式回填本工单,并明确 ADR-0001 是否成立;若不成立,列出回退方案(切换被测分组 + /connections 链路校验,含测速竞争中止逻辑)对 02~04 号工单的具体影响清单并更新 spec。

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] 结论明确:listener 绑 GLOBAL + PUT GLOBAL 切换/恢复是否可行,附验证证据(出口 IP 对照等)
- [x] 若不可行:回退方案的变更影响清单已列出,spec 的 Implementation Decisions 已同步更新(不适用:结论可行)
- [x] 验证用的临时配置、脚本、进程已清理干净

## Answer

**结论:ADR-0001 机制成立,无需回退。** 验证环境:mihomo v1.19.31(linux amd64),最小配置含一条 `echo-proxy`(type http → 本地回环代理)+ `MATCH,DIRECT` 规则 + listener `speedtest-in`(mixed,127.0.0.1:7891,`proxy: GLOBAL`)。用本地回环代理 + 目标服务器替代真实节点(观察"流量是否经过被选适配器",证据等价于出口 IP 对照)。

验证结果:

1. **listener 绑 GLOBAL 合法且生效**:启动无报错,日志 `Mixed(http+socks)[speedtest-in] proxy listening at: 127.0.0.1:7891`。
2. **切换生效**:`PUT /proxies/GLOBAL {"name":"echo-proxy"}` → 204;经 7891 下载 2MB 成功,mihomo 路由日志 `using GLOBAL[echo-proxy]`,回环代理记录到 CONNECT,实测吞吐 ~116 MB/s(回环),字节计数正确。
3. **规则流量隔离**:GLOBAL=echo-proxy 期间,经规则端口(命中 `MATCH,DIRECT`)下载 1MB 成功且回环代理日志零新增 → 规则模式的用户流量完全不经过 GLOBAL,不受切换影响。
4. **恢复干净**:`PUT /proxies/GLOBAL {"name":"DIRECT"}` → 204,`now` 回到 DIRECT;此后经 7891 的下载日志为 `using GLOBAL[DIRECT]`,回环代理零新增。

**对实现的补充约束(来自 spike 观察)**:

- mihomo 会把 GLOBAL 的选择持久化到其数据目录的 `cache.db`,进程重启后残留。因此测速命令的恢复逻辑必须**无条件执行**(下载超时/panic 路径也要恢复),不能依赖进程内记忆。
- 残留的 GLOBAL `now` 对规则模式无害(规则流量不消费 GLOBAL),但实现仍以保证"测速结束即恢复"为准。
- 验证脚本与配置在 `/tmp/speedtest-spike/`(仓库外),进程与 cache.db 已清理。

## Comments

- 2026-09-30:spike 由主会话执行并回填(替代原计划的 implementer 子代理——机制验证需要即时排障,涉及两次工具脚本 bug 修复,详见过程;不影响结论)。
