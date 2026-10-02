# 08: CI 打包接线 + 文档收尾

**What to build:** Windows 构建管线接入 meow 二进制:测试包工作流(win-x64-test-build)与正式版工作流(fork-release)构建时按仓库里钉死的版本号从 meow-rs 官方 release 下载 `x86_64-pc-windows-msvc` zip,解出 meow.exe + wintun.dll 作为第二 sidecar 打进安装包(二进制不进 git)。文档收尾:双内核架构与测速 IN-PORT 适配两篇 ADR,AGENTS.md 功能清单登记。本票交付后,其余工单的 Windows 实测才有安装包可用。

**Blocked by:** 02(sidecar 命名、版本钉位、下载形态在 02 定型)

**Status:** ready-for-agent

- [ ] 测试包工作流出包成功,安装包含 meow.exe 与 wintun.dll,离线 Windows 机器安装即用
- [ ] fork-release 工作流出包成功(同上)
- [ ] meow 版本号钉在仓库单一位置,bump 只改一处,构建可重现
- [ ] ADR:双内核架构、测速 IN-PORT 适配(若 01 定案回退,则写回退决策)
- [ ] AGENTS.md 功能清单登记 meow-rs 第二内核
- [ ] 验证门通过:`pnpm typecheck && pnpm test` 与 `src-tauri` 下 `cargo check`
