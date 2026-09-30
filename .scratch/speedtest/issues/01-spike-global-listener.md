# 01: Spike:验证 GLOBAL 专用测速通道

**What to build:** 不含产品 UI 的机制验证(对应 spec 与 ADR-0001):临时在 mihomo 配置中注入一个仅绑定 127.0.0.1 的 mixed listener,`proxy` 字段绑定为 GLOBAL;通过 REST API 把 GLOBAL 切到某个被测节点,从该端口发起 HTTP 限时下载,验证三件事:① 下载流量出口确为被测节点(可对照切换前后的出口 IP);② 规则模式下用户其他流量不受影响;③ PUT GLOBAL 恢复原值后一切如初。验证结论以 Comments 形式回填本工单,并明确 ADR-0001 是否成立;若不成立,列出回退方案(切换被测分组 + /connections 链路校验,含测速竞争中止逻辑)对 02~04 号工单的具体影响清单并更新 spec。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 结论明确:listener 绑 GLOBAL + PUT GLOBAL 切换/恢复是否可行,附验证证据(出口 IP 对照等)
- [ ] 若不可行:回退方案的变更影响清单已列出,spec 的 Implementation Decisions 已同步更新
- [ ] 验证用的临时配置、脚本、进程已清理干净
