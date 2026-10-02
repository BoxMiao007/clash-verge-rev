# 02: 平级切换打通

**What to build:** 设置页内核选择出现 meow-rs 第三项,与 mihomo 平级一键切换:切到 meow 后 sidecar 模式下系统代理可用、能上网;切回 mihomo 无损;切换失败自动回滚并提示。这是第一个纵向切片——内核枚举扩展、meow 二进制接入(dev 下载脚本 + sidecar 声明,版本号钉住)、切换命令校验/回滚、设置页选项、版本号显示一次打通。默认内核仍是 mihomo,升级 fork 不改变用户当前选择。

**Blocked by:** 01(spike 的 CLI 兼容性与 warn-skip 结论决定切换实现细节)

**Status:** ready-for-agent

- [ ] 设置页内核弹窗显示 meow-rs 第三项,当前内核高亮正确
- [ ] 切到 meow:配置校验通过、内核重启、系统代理可用、浏览器经代理上网
- [ ] 切回 mihomo:订阅与分组选择不丢,代理恢复
- [ ] 人为制造切换失败(配置校验不过)自动回滚,用户看到明确提示而非断网
- [ ] 关于/设置页显示当前 meow 内核版本号
- [ ] WSL dev 环境用 Linux 版 meow sidecar 可完整复现上述流程
- [ ] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`
