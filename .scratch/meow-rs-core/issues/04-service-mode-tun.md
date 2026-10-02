# 04: 服务模式 + TUN

**What to build:** Clash Verge Service 以托管 mihomo 相同的方式托管 meow.exe(传 `-d` 数据目录与 `-f` 配置路径),服务模式下 Windows TUN 可用(wintun.dll 随安装包分发)。用户视角:服务模式安装/卸载、启停、TUN 开关在 meow 下与 mihomo 体验无差。开发可在 WSL 先行验证 sidecar 路径,最终验收依赖工单 08 产出的 Windows 测试包实测。

**Blocked by:** 02、03(fake-ip 强制是 TUN 的前提);最终 Windows 实测需 08 的测试包(不阻塞开发)

**Status:** ready-for-agent

- [ ] 服务模式启动 meow 成功,启停/切换/卸载与 mihomo 行为一致
- [ ] 服务模式 + TUN:Windows 实测全部流量走 TUN 且可上网,DNS 解析正常(fake-ip 生效)
- [ ] sidecar(无服务)模式下 meow 行为不变(回归)
- [ ] 服务日志可查,内核异常退出有明确报错
- [ ] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`
