# 01: Spike:meow 实证验证

**What to build:** 在 WSL 用官方 Linux 版 meow 二进制完成 spec「Further Notes」清单的 5 项实证,结论落档。本票不产出产品功能,产出**事实**:每项验证的实测命令、观察结果与结论,直接决定 03(配置补丁)与 05(测速适配)的实现方式。验证完成后测速适配方案定案:IN-PORT 引流可行则按原方案走,否则定案回退为「meow 下测速入口灰显」。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 官方 Linux 版 meow 下载可用,`-t -d <dir> -f <config>` 校验通过、能正常启动代理
- [ ] `IN-PORT,<专用端口>,GLOBAL` 规则能把该端口流量引向 GLOBAL 组(mixed listener + rule 模式实测,下载流量走被测节点)
- [ ] `PUT /proxies/GLOBAL` 可切换 GLOBAL 选择,且选择跨进程重启持久化
- [ ] `/traffic`、`/logs`、`/connections`、`/memory` 四个 WS 端点按 mihomo 契约推送(verge 前端客户端可消费的数据形态)
- [ ] mihomo 专有键喂给 meow(非 strict)的实际行为确认为 warn-and-skip
- [ ] 5 项结论(含失败项与替代方案)记入 `.scratch/meow-rs-core/`,测速适配方案定案并回填 spec
