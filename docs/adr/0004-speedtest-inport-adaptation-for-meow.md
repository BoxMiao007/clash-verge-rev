# 0004 - 下载测速在 meow 内核下改用 IN-PORT 规则引流

ADR-0001 的测速通道依赖 mihomo 的 listener `proxy: GLOBAL` 绑定——专用 mixed listener 在配置里直接把自身流量绑到 GLOBAL 组。spike 实证该字段是 mihomo 专有:meow 对它**静默忽略**(带 `proxy: GLOBAL` 启动、GLOBAL 切到死代理后,专用端口流量仍走默认路由成功),mihomo 下的通道形态在 meow 下不存在。决定:meow 激活时配置生成末端改为**规则引流**——专用 listener 保持 `type: mixed` 且不带 `proxy` 字段,在规则栈顶注入 `IN-PORT,<测速专用端口>,GLOBAL`。死代理三段对照实证(spike 项 b)该规则把端口流量强制推进 GLOBAL 且不受默认路由干扰(删掉规则即回到默认路由),`GET /connections` 显示连接 `chains:["GLOBAL"]、rule:"IN-PORT"` 与 mihomo 绑定形态同源;GLOBAL 选择「切换即落盘、跨重启恢复」(spike 项 c)保证 ADR-0001 修订引入的恢复日志机制在 meow 下继续成立。测速命令的「切 GLOBAL → 限时下载 → 恢复」流程、GLOBAL 读写接缝与恢复日志跨内核原样复用,内核分叉只发生在配置注入侧;meow 分支注入时同时删除 listener 残留的 `proxy:` 字段,避免留下看似有效实则被忽略的键。

## Considered Options

- 向 meow-rs 上游提 PR 补 listener `proxy:` 绑定:对齐 mihomo 语义当然最干净,但上游交付时间不可控,不作为主路径(spec 明确 Out of Scope);上游若支持后可整体收回本适配。
- meow 下灰显禁用测速按钮:spike 前的保底回退方案,实现成本最低;IN-PORT 引流实证可行后弃用——下载测速是 fork 的标志性功能,无功能理由在 meow 下缩水。
- 复用主 mixed 端口测速(切 GLOBAL 后从主端口发起):mihomo 下已因「用户正常流量在测速窗口内被一并推进 GLOBAL」而弃用(ADR-0001 引入专用 listener 的原因),meow 下同病,不随内核切换翻案。
